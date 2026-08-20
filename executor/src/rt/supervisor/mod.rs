use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    sync::{atomic::AtomicU32, Arc},
};

use anyhow::Context as _;
use genvm_common::internal_constants::{memory_limiter_consts, top_limits};
use genvm_common::*;

use crate::int_traits::*;
use crate::{
    config, host, public_abi,
    rt::{self, DetNondet},
    runners, wasi,
};

pub mod actions;
mod compilation;

struct WasmModuleCache {
    cache_dir: Option<std::path::PathBuf>,
    wasm_modules_cache: sync::CacheMap<DetNondet<wasmtime::Module>>,
}

pub struct NonDetVMTask {
    pub task: Box<wasi::genlayer_sdk::SingleVMData>,
    pub call_no: u32,
    pub tasks_done: Arc<tokio::sync::Notify>,
}

impl NonDetVMTask {
    pub async fn run_now(self, sup: &Arc<Supervisor>) -> anyhow::Result<rt::vm::RunOk> {
        run_single_nondet(sup, self).await
    }
}

pub struct VMCountDecrementer(Arc<Supervisor>);

impl std::ops::Drop for VMCountDecrementer {
    fn drop(&mut self) {
        self.0.queue.vm_countdown.decrement();
    }
}

struct NondetQueue {
    sender: tokio_mpmc::Sender<sync::Lock<NonDetVMTask, VMCountDecrementer>>,
    receiver: tokio_mpmc::Receiver<sync::Lock<NonDetVMTask, VMCountDecrementer>>,
    nondet_call_disagree: std::sync::atomic::AtomicU32,
    vm_countdown: genvm_common::sync::Waiter,
    tasks_loop_done: Arc<tokio::sync::RwLock<()>>,
    encountered_error: crossbeam::atomic::AtomicCell<Option<anyhow::Error>>,
}

pub struct Ctor {
    pub shared_data: sync::DArc<rt::SharedData>,

    pub modules: crate::modules::All,

    pub locked_slots: host::LockedSlotsSet,
    pub leader_nondet_results: Option<Vec<bytes::Bytes>>,
    pub multi_host: host::MultiHost,
    pub record_actions: Vec<String>,
}

#[derive(Clone, Debug, genlayer_calldata::Encode)]
pub struct RecordedAction {
    pub kind: String,
    pub fields: BTreeMap<String, String>,
}

pub struct ActionRecorder {
    enabled: BTreeSet<String>,
    records: tokio::sync::Mutex<Vec<RecordedAction>>,
}

impl ActionRecorder {
    fn new(enabled: Vec<String>) -> Self {
        Self {
            enabled: enabled.into_iter().collect(),
            records: tokio::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn is_enabled(&self, kind: &str) -> bool {
        self.enabled.contains(kind)
    }

    pub async fn record_or_log(&self, kind: &'static str, fields: BTreeMap<String, String>) {
        if self.is_enabled(kind) {
            self.records.lock().await.push(RecordedAction {
                kind: kind.to_owned(),
                fields,
            });
        } else {
            log_debug!(kind = kind, fields:serde = fields; "auditable action");
        }
    }

    pub async fn take(&self) -> Vec<RecordedAction> {
        std::mem::take(&mut *self.records.lock().await)
    }
}

pub struct Supervisor {
    pub shared_data: sync::DArc<rt::SharedData>,
    pub modules: crate::modules::All,
    pub locked_slots: host::LockedSlotsSet,

    pub nondet_call_no: AtomicU32,
    pub balances: dashmap::DashMap<calldata::Address, primitive_types::U256>,
    pub nondet_results: tokio::sync::Mutex<Vec<bytes::Bytes>>,
    pub leader_nondet_results: Option<Vec<bytes::Bytes>>,

    queue: NondetQueue,
    runner_cache: runners::cache::Reader,
    wasm_mod_cache: WasmModuleCache,

    /// Weak registry of runtime-registered `custom:<hash>` runners, keyed by
    /// canonical id. Holds only weak references -- a registration lives while some
    /// VM's loaded set pins it. Pure cross-scope dedup: resolution
    /// never reads it, so a scope can only use what it loaded.
    custom_runners: runners::cache::WeakCache,

    pub(crate) engines: rt::DetNondet<wasmtime::Engine>,
    pub(crate) host: Arc<host::MultiHost>,
    pub(crate) action_recorder: ActionRecorder,
}

#[cfg(debug_assertions)]
impl Drop for Supervisor {
    fn drop(&mut self) {
        // Every VM (and thus every loaded-set pin) is gone by supervisor teardown,
        // so no registered custom runner may still be resident.
        self.custom_runners
            .assert_empty_on_teardown("custom runner");
    }
}

const fn get_native_stack_size() -> u32 {
    let native_stack_size = 6_u32 << 20;

    let approximation =
        top_limits::WASM_STACK_VALUE_SLOTS * 8 * 4 + top_limits::WASM_CALL_DEPTH * 64;

    if native_stack_size < approximation {
        panic!("native stack size is smaller than the configured call depth limit");
    }

    native_stack_size
}

pub fn create_engines(
    config_base: impl FnOnce(&mut wasmtime::Config) -> anyhow::Result<()>,
) -> anyhow::Result<rt::DetNondet<wasmtime::Engine>> {
    let mut base_conf = wasmtime::Config::default();

    base_conf
        .debug_info(true)
        .wasm_backtrace_details(wasmtime::WasmBacktraceDetails::Disable)
        .consume_fuel(false)
        .cranelift_opt_level(wasmtime::OptLevel::None)
        .async_stack_size(8 << 20)
        .max_wasm_stack(get_native_stack_size().into_int_comptime())
        .wasm_stack_limits(
            top_limits::WASM_CALL_DEPTH,
            top_limits::WASM_STACK_VALUE_SLOTS,
        );

    base_conf
        .wasm_tail_call(true)
        .wasm_bulk_memory(true)
        .wasm_simd(true)
        .relaxed_simd_deterministic(true)
        .wasm_relaxed_simd(false);

    use wasmparser::WasmFeatures;

    base_conf
        .wasm_features(WasmFeatures::BULK_MEMORY, true)
        .wasm_features(WasmFeatures::SIGN_EXTENSION, true)
        .wasm_features(WasmFeatures::MUTABLE_GLOBAL, true)
        .wasm_features(WasmFeatures::MULTI_VALUE, true)
        .wasm_features(WasmFeatures::SATURATING_FLOAT_TO_INT, false)
        //.wasm_features(WasmFeatures::REFERENCE_TYPES, false)
        .wasm_features(WasmFeatures::SATURATING_FLOAT_TO_INT, true);

    config_base(&mut base_conf)?;

    let mut det_conf = base_conf.clone();
    det_conf
        .wasm_floats_enabled(false)
        .cranelift_nan_canonicalization(true)
        .wasm_backtrace(true);

    let mut non_det_conf = base_conf.clone();
    non_det_conf.wasm_floats_enabled(true).wasm_backtrace(false);

    let det_engine = wasmtime::Engine::new(&det_conf)
        .map_err(crate::wasmtime_to_anyhow)
        .with_context(|| "creating deterministic wasm engine")?;
    let non_det_engine = wasmtime::Engine::new(&non_det_conf)
        .map_err(crate::wasmtime_to_anyhow)
        .with_context(|| "creating non-deterministic wasm engine")?;

    Ok(rt::DetNondet {
        det: det_engine,
        non_det: non_det_engine,
    })
}

pub async fn await_nondet_vms(zelf: &Arc<Supervisor>) -> anyhow::Result<Option<u32>> {
    zelf.queue.sender.close(); // no more tasks can be submitted after this point

    zelf.queue.vm_countdown.decrement();

    if !zelf.queue.receiver.is_empty() {
        let read_permit = zelf
            .queue
            .tasks_loop_done
            .clone()
            .try_read_owned()
            .expect("tasks_loop_done already held by writer");
        // Each queued nondet VM carries its own granted custom-runner pins and
        // pays for them via its spawn-time inherit load actions.
        // Nondet VMs do not inherit the deterministic limiter; each starts with
        // a fresh RAM limiter when spawned.
        nondet_vm_processor(zelf.clone(), read_permit).await;
    }

    let _ = zelf.queue.tasks_loop_done.write().await;

    log_debug!("all nondet workers done");

    if let Some(err) = zelf.queue.encountered_error.take() {
        return Err(err);
    }

    let disagree_call = zelf
        .queue
        .nondet_call_disagree
        .load(std::sync::atomic::Ordering::SeqCst);
    if disagree_call == u32::MAX {
        return Ok(None);
    }

    Ok(Some(disagree_call))
}

pub async fn submit_nondet_vm_task(zelf: &Arc<Supervisor>, task: NonDetVMTask) {
    let call_no = task.call_no;

    zelf.queue.vm_countdown.increment();
    let tok = VMCountDecrementer(zelf.clone());
    let _ = zelf
        .queue
        .sender
        .send(sync::Lock::new(task, tok))
        .await
        .inspect_err(|e| {
            log_error!(error:err = e; "failed to submit nondet vm task");
        });

    log_debug!(call_no = call_no; "nondet vm task submitted");
}

impl Supervisor {
    pub async fn push_nondet_result(&self, call_no: u32, result: rt::vm::ContractResultBytes) {
        let mut vec = self.nondet_results.lock().await;
        let idx = u32_into_usize(call_no);
        while vec.len() <= idx {
            vec.push(bytes::Bytes::new());
        }
        vec[idx] = result.into_bytes();
    }

    pub async fn take_nondet_results(&self) -> Vec<bytes::Bytes> {
        self.nondet_results.lock().await.clone()
    }

    pub async fn take_recorded_actions(&self) -> Vec<RecordedAction> {
        self.action_recorder.take().await
    }

    pub fn get_leader_nondet_result(&self, call_no: u32) -> Option<bytes::Bytes> {
        self.leader_nondet_results
            .as_ref()
            .and_then(|v| v.get(u32_into_usize(call_no)).cloned())
    }

    pub fn get_storage_limiter(&self) -> rt::vm::storage::Limiter {
        rt::vm::storage::Limiter::new(self.shared_data.gep(|x| &x.data_fees_limit))
    }

    /// Inserts the just-deployed contract's runner into the cache and returns a
    /// pin. The cache is weakly held, so the caller (the top-level execution
    /// scope in `lib.rs`) must keep the pin for the whole run -- otherwise the
    /// entry would be evicted before the contract VM loads it.
    #[must_use]
    pub fn prepopulate_deploy_runner(
        &self,
        address: calldata::Address,
        code_slot: crate::SlotID,
        archive: runners::Archive,
    ) -> runners::cache::ArchivePin {
        let id = runners::Id::Chain {
            address,
            on: runners::ChainState::Deploy,
            slot: code_slot,
        }
        .canonical();
        self.runner_cache.put(id, archive)
    }

    pub fn start(config: &config::Config, ctor: Ctor) -> anyhow::Result<Arc<Self>> {
        let my_cache_dir = runners::cache::get_cache_dir(&config.cache_dir).ok();

        let engines = create_engines(|base_conf| {
            match &my_cache_dir {
                None => {
                    base_conf.cache(None);
                }
                Some(cache_dir) => {
                    let mut cache_dir = cache_dir.to_owned();
                    cache_dir.push("wasmtime");

                    let cache_conf: wasmtime_cache::CacheConfig =
                        serde_json::from_value(serde_json::Value::Object(
                            [(
                                "directory".into(),
                                cache_dir.into_os_string().into_string().unwrap().into(),
                            )]
                            .into_iter()
                            .collect(),
                        ))
                        .context("creating cache config")?;
                    base_conf
                        .cache_config_set(cache_conf)
                        .map_err(crate::wasmtime_to_anyhow)
                        .context("setting cache config")?;
                }
            }
            Ok(())
        })
        .context("creating wasmtime engines")?;

        let (sender, receiver) = tokio_mpmc::channel(100);

        let debug_mode = ctor.shared_data.debug_mode;

        let zelf = Arc::new(Self {
            shared_data: ctor.shared_data,
            modules: ctor.modules,
            locked_slots: ctor.locked_slots,
            nondet_call_no: AtomicU32::new(0),
            balances: dashmap::DashMap::new(),
            nondet_results: Default::default(),
            leader_nondet_results: ctor.leader_nondet_results,
            queue: NondetQueue {
                sender,
                receiver,
                encountered_error: crossbeam::atomic::AtomicCell::new(None),
                nondet_call_disagree: std::sync::atomic::AtomicU32::new(u32::MAX),
                vm_countdown: genvm_common::sync::Waiter::new(),
                tasks_loop_done: Arc::new(tokio::sync::RwLock::new(())),
            },
            runner_cache: runners::cache::Reader::new(
                std::path::Path::new(&config.runners_dir),
                std::path::Path::new(&config.registry_dir),
                debug_mode.allows_latest_resolution(),
            )
            .context("creating runner cache reader")?,
            wasm_mod_cache: WasmModuleCache {
                cache_dir: my_cache_dir,
                wasm_modules_cache: sync::CacheMap::new(),
            },
            custom_runners: runners::cache::WeakCache::new(),
            host: Arc::new(ctor.multi_host),
            engines,
            action_recorder: ActionRecorder::new(ctor.record_actions),
        });

        let read_permit = zelf
            .queue
            .tasks_loop_done
            .clone()
            .try_read_owned()
            .expect("tasks_loop_done already held by writer");
        tokio::spawn(nondet_vm_processor(zelf.clone(), read_permit));

        Ok(zelf)
    }
}

pub async fn spawn(
    zelf: &Arc<Supervisor>,
    vm: Box<wasi::genlayer_sdk::SingleVMData>,
) -> std::result::Result<rt::vm::VM<()>, rt::SpawnError> {
    if vm.remaining_recursion == 0 {
        return Err(rt::SpawnError {
            error: rt::errors::Error::vm(public_abi::VmError::out_of().vm_recursion()).into(),
            state: Box::new(rt::SpawnErrorState::Unspawned(vm)),
        });
    }

    if !vm.limiter.consume(memory_limiter_consts::VM_SPAWN_COST) {
        return Err(rt::SpawnError {
            error: rt::errors::Error::vm(crate::public_abi::VmError::out_of().memory().val())
                .into(),
            state: Box::new(rt::SpawnErrorState::Unspawned(vm)),
        });
    }

    zelf.action_recorder
        .record_or_log(
            "vm_spawn",
            BTreeMap::from([
                ("spawn_kind".to_owned(), vm.spawn_kind.clone()),
                ("depth".to_owned(), vm.depth().to_string()),
                (
                    "runner_id".to_owned(),
                    vm.conf.execution.topmost_runner_id.to_string(),
                ),
            ]),
        )
        .await;

    let config_copy = vm.conf.clone();

    let engine = zelf.engines.get(vm.conf.permissions.deterministic);

    let limiter = vm.limiter.clone();
    let mut store = wasmtime::Store::new(
        engine,
        rt::vm::WasmtimeStoreData {
            limits: vm.limiter.clone(),
            genlayer_ctx: wasi::Context::new(vm, limiter).map_err(|(a, b)| rt::SpawnError {
                error: anyhow::Error::from(a),
                state: Box::new(rt::SpawnErrorState::Unspawned(b)),
            })?,
        },
        wasmtime::GenVMCtx {
            // The executor has no cooperative cancellation; the manager kills the
            // process on timeout. wasmtime still requires this flag, so feed it a
            // never-set atomic.
            should_quit: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        },
    );

    store.limiter(|ctx| &mut ctx.limits);

    let mut vm_base = rt::vm::VMBase {
        store,
        linker: wasmtime::Linker::new(engine),
        config_copy,
    };

    vm_base.linker.allow_unknown_exports(false);
    vm_base.linker.allow_shadowing(false);

    if let Err(e) = wasi::add_to_linker_sync(
        &mut vm_base.linker,
        |host: &mut rt::vm::WasmtimeStoreData| host.genlayer_ctx_mut(),
    ) {
        return Err(rt::SpawnError {
            error: e,
            state: Box::new(rt::SpawnErrorState::Spawned(Box::new(vm_base))),
        });
    }

    // Inherit-at-spawn: perform a load action for each custom
    // runner granted to this child, charging its own limiter, *before* its main
    // runner loads (so a custom entry point granted here is not charged twice).
    // The pins were carried in `SingleVMData`, so the content survives even if the
    // granting parent has died (queued nondet VMs).
    {
        let is_det = vm_base.config_copy.permissions.deterministic;
        let child_limiter = vm_base.store.data().limits.clone();
        let store_data = vm_base.store.data_mut();
        let wasi::genlayer_sdk::Context { loaded, data, .. } =
            &mut store_data.genlayer_ctx.genlayer_sdk;
        let grants = std::mem::take(&mut data.granted_custom);
        let mut det_fingerprint = is_det.then_some(&mut data.det_subvm_hashes);
        for grant in grants {
            let runner_id = grant.runner_id();
            let size = grant.total_size();
            let status = if loaded.contains(runner_id) {
                "cached"
            } else {
                "charged"
            };
            if let Err(e) = actions::inherit_load(
                &child_limiter,
                loaded,
                det_fingerprint.as_deref_mut(),
                grant,
            ) {
                return Err(rt::SpawnError {
                    error: e,
                    state: Box::new(rt::SpawnErrorState::Spawned(Box::new(vm_base))),
                });
            }
            zelf.action_recorder
                .record_or_log(
                    "runner_load",
                    BTreeMap::from([
                        ("runner_id".to_owned(), runner_id.as_str().to_owned()),
                        ("size".to_owned(), size.to_string()),
                        ("status".to_owned(), status.to_owned()),
                    ]),
                )
                .await;
        }
    }

    Ok(rt::vm::VM { vm_base, data: () })
}

pub async fn apply_contract_actions(
    zelf: &std::sync::Arc<Supervisor>,
    mut vm: rt::vm::VM<()>,
) -> std::result::Result<rt::vm::VM<wasmtime::Instance>, rt::SpawnError> {
    let limiter = vm.vm_base.store.data_mut().limits.clone();

    let res = apply_contract_actions_inner(zelf, &mut vm, limiter).await;

    match res {
        Ok(inst) => Ok(rt::vm::VM {
            vm_base: vm.vm_base,
            data: inst,
        }),
        Err(e) => Err(rt::SpawnError {
            error: e,
            state: Box::new(rt::SpawnErrorState::Spawned(Box::new(vm.vm_base))),
        }),
    }
}

async fn apply_contract_actions_inner(
    zelf: &std::sync::Arc<Supervisor>,
    vm: &mut rt::vm::VM<()>,
    limiter: rt::memlimiter::Limiter,
) -> anyhow::Result<wasmtime::Instance> {
    let data = &mut vm.vm_base.store.data_mut().genlayer_ctx.genlayer_sdk.data;

    let topmost_runner_id = data.conf.execution.topmost_runner_id.clone();
    let contract_major = data
        .storage
        .read_major()
        .await
        .with_context(|| format!("reading contract major for {topmost_runner_id}"))?;
    let node_major = genvm_common::version::CURRENT.major;
    if u16::from(contract_major) != node_major {
        return Err(rt::errors::Error::wrap(
            public_abi::VmError::invalid_contract().major_mismatch(),
            anyhow::anyhow!("contract major {contract_major} != node major {node_major}"),
        )
        .into());
    }

    let topmost_runner_id = data.conf.execution.topmost_runner_id.clone();

    // Main-runner load action. Runs *after* the spawn-time inherit
    // loads, so a custom entry point granted to this VM is already loaded (free
    // here, not double charged). Charges this VM's own limiter.
    let arch = {
        let is_det = vm.vm_base.config_copy.permissions.deterministic;
        let store_data = vm.vm_base.store.data_mut();
        let wasi::genlayer_sdk::Context { loaded, data, .. } =
            &mut store_data.genlayer_ctx.genlayer_sdk;
        let det_fingerprint = is_det.then_some(&mut data.det_subvm_hashes);
        actions::load_action(
            zelf,
            &limiter,
            loaded,
            det_fingerprint,
            topmost_runner_id.clone().into(),
        )
        .await
        .with_context(|| format!("getting runner for {topmost_runner_id}"))?
    };

    let actions = arch
        .get_actions()
        .await
        .with_context(|| format!("loading init actions for contract {topmost_runner_id}"))
        .map_err(|e| rt::errors::Error::wrap(public_abi::VmError::invalid_contract().val(), e))?;

    let mut ctx = actions::Ctx {
        env: runners::Env::new(limiter.clone()),
        visited: HashSet::from([topmost_runner_id.canonical()]),
        topmost_runner_id: topmost_runner_id.clone(),
        supervisor: zelf,
        vm: &mut vm.vm_base,
    };

    let inst = match ctx
        .apply(&actions, topmost_runner_id.canonical(), &arch)
        .await
    {
        Ok(Some(inst)) => inst,
        Ok(None) => {
            return Err(runners::malformed_runner_error(format!(
                "init actions for contract {topmost_runner_id} have no start instruction"
            ))
            .into());
        }
        Err(e) => {
            return Err(
                rt::errors::Error::wrap(public_abi::VmError::invalid_contract().val(), e).into(),
            );
        }
    };

    Ok(inst)
}

async fn run_single_nondet(
    zelf: &std::sync::Arc<Supervisor>,
    task: NonDetVMTask,
) -> anyhow::Result<rt::vm::RunOk> {
    let zelf = zelf.clone();
    let res = tokio::task::spawn(async move { run_single_nondet_inner(&zelf, task).await })
        .await
        .map_err(|e| anyhow::anyhow!("nondet VM task failed to join: {e}"))?;

    match res {
        Ok(v) => Ok(v.run_ok),
        Err(e) => rt::errors::unwrap_vm_errors(rt::errors::UnwrapDynError::from(e)),
    }
}

async fn run_single_nondet_inner(
    zelf: &std::sync::Arc<Supervisor>,
    task: NonDetVMTask,
) -> anyhow::Result<rt::vm::RunResult> {
    rt::spawn_apply_run(zelf, task.task).await
}

async fn nondet_vm_processor(
    zelf: std::sync::Arc<Supervisor>,
    read_permit: tokio::sync::OwnedRwLockReadGuard<()>,
) {
    let mut count = 0;
    loop {
        tokio::select! {
            _ = zelf.queue.vm_countdown.wait() => {
                log_debug!("vm countdown reached zero, stopping nondet validator queue");
                break;
            }

            Ok(val) = zelf.queue.receiver.recv() => {
                let Some(task) = val else {
                    log_debug!("nondet vm processor: all senders closed, exiting");
                    break;
                };
                count += 1;

                let task_done = task.tasks_done.clone();

                let _dropper = sync::DropGuard::new(move || {
                    task_done.notify_one();
                });

                if zelf.queue.nondet_call_disagree.load(std::sync::atomic::Ordering::SeqCst) != u32::MAX {
                    log_info!("skipped nondet block due to disagreement in previous one");

                    continue;
                }

                let call_no = task.call_no;

                let (task, tok) = task.deconstruct();
                let res = run_single_nondet(&zelf, task).await;

                let do_disagree = match res {
                    Ok(rt::vm::RunOk::Return(v)) => {
                        match v.as_bool() {
                            None => {
                                log_warn!("nondet block returned non-bool value, setting to disagree");
                                true
                            },
                            Some(b) => !b,
                        }
                    },
                    Ok(rt::vm::RunOk::FatalVMError(e, cause)) => {
                        let e: anyhow::Error = rt::errors::Error::fatal_vm_cause(e, cause).into();
                        if let Some(old_err) = zelf.queue.encountered_error.swap(Some(e)) {
                            log_error!(error:ah = old_err; "encountered another error, overwriting");
                        }
                        continue;
                    },
                    Ok(other) => {
                        log_warn!(result:? = other; "unexpected result in nondet block, setting to disagree");
                        true
                    }
                    Err(e) => {
                        if let Some(old_err) = zelf.queue.encountered_error.swap(Some(e)) {
                            log_error!(error:ah = old_err; "encountered another error, overwriting");
                        }
                        continue;
                    }
                };

                log_trace!(call_no = call_no, do_disagree = do_disagree; "nondet call result");

                if do_disagree {
                    zelf.queue.nondet_call_disagree
                        .fetch_min(call_no, std::sync::atomic::Ordering::SeqCst);
                }

                std::mem::drop(tok);
            }
        }
    }

    std::mem::drop(read_permit);
    log_debug!(count = count; "nondet worker done");
}
