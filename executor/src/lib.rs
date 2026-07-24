pub mod caching;
pub mod config;
pub mod domain;
pub mod host;
pub mod modules;
pub mod rt;
pub mod runners;
pub mod wasi;

use genlayer_sdk::abi;
pub use genlayer_sdk::abi::consts as public_abi;

pub use genlayer_sdk::calldata;
use genvm_common::*;

pub use host::{Host, SlotID};

use anyhow::{Context, Result};
use wasi::genlayer_sdk::ExtendedMessage;

use std::sync::Arc;

use crate::wasi::genlayer_sdk::VMDataAccumulator;

pub fn wasmtime_to_anyhow(e: wasmtime::Error) -> anyhow::Error {
    match e.downcast() {
        Ok(anyhow_e) => anyhow_e,
        Err(e) => e.into(),
    }
}

pub fn anyhow_to_wasmtime(e: anyhow::Error) -> wasmtime::Error {
    match e.downcast() {
        Ok(wasmtime_e) => wasmtime_e,
        Err(e) => wasmtime::Error::from_anyhow(e),
    }
}

#[derive(Default, Debug, serde::Serialize)]
pub struct Metrics {
    pub supervisor: rt::Metrics,
    pub web_module: modules::Metrics,
    pub llm_module: modules::Metrics,
    pub hosts: Box<[host::Metrics]>,
}

impl<W: calldata::Writer> calldata::codec::Encode<W> for Metrics {
    type Error = W::Error;

    fn encode(&self, enc: &mut calldata::Encoder<W>) -> Result<(), Self::Error> {
        enc.start_map(4)?;
        enc.push_map_k("hosts")?;
        enc.start_array(self.hosts.len() as u64)?;
        for h in self.hosts.iter() {
            calldata::codec::Encode::encode(h, enc)?;
        }
        enc.push_map_k("llm_module")?;
        calldata::codec::Encode::encode(&self.llm_module, enc)?;
        enc.push_map_k("supervisor")?;
        calldata::codec::Encode::encode(&self.supervisor, enc)?;
        enc.push_map_k("web_module")?;
        calldata::codec::Encode::encode(&self.web_module, enc)?;
        Ok(())
    }
}

pub struct CreateSupervisorNamedArgs {
    pub method_hosts: Vec<u8>,
    pub gas_data: std::collections::BTreeMap<String, String>,
    pub initial_time_units_allocation: u32,
    pub leader_nondet_results: Option<Vec<bytes::Bytes>>,
    pub record_actions: Vec<String>,
}

pub struct ExecutionContext {
    pub supervisor: Arc<rt::supervisor::Supervisor>,
    det_limiter: rt::memlimiter::Limiter,
}

pub fn create_supervisor(
    config: &config::Config,
    mut hosts: Vec<Host>,
    named: CreateSupervisorNamedArgs,
    host_data: genvm_modules_interfaces::HostData,
    shared_data: sync::DArc<rt::SharedData>,
    message: &genvm_modules_interfaces::MessageData,
) -> Result<ExecutionContext> {
    let metrics = shared_data.gep(|x| &x.metrics);

    let role = if named.leader_nondet_results.is_none() {
        genvm_modules_interfaces::Role::Leader
    } else {
        genvm_modules_interfaces::Role::Validator
    };

    let modules = modules::All {
        web: Arc::new(modules::Module::new(
            modules::ModuleNamedArgs {
                name: "web".into(),
                url: config.modules.web.address.clone(),
                gas_data: named.gas_data.clone(),
                initial_time_units_allocation: named.initial_time_units_allocation,
            },
            shared_data.genvm_id,
            role,
            host_data.clone(),
            metrics.gep(|x| &x.web_module),
        )),
        llm: Arc::new(modules::Module::new(
            modules::ModuleNamedArgs {
                name: "llm".into(),
                url: config.modules.llm.address.clone(),
                gas_data: named.gas_data,
                initial_time_units_allocation: named.initial_time_units_allocation,
            },
            shared_data.genvm_id,
            role,
            host_data,
            metrics.gep(|x| &x.llm_module),
        )),
    };

    // has locked slots
    let limiter_det = rt::memlimiter::Limiter::new();

    let storage_host_idx =
        if (host::host_fns::Methods::StorageRead as usize) < named.method_hosts.len() {
            named.method_hosts[host::host_fns::Methods::StorageRead as usize] as usize
        } else {
            0
        };

    let locked_slots = hosts[storage_host_idx]
        .get_locked_slots_for_sender(
            calldata::Address::from(message.contract_address.raw()),
            calldata::Address::from(message.sender_address.raw()),
            &limiter_det,
        )
        .context("reading locked slots")?;

    let multi_host = host::MultiHost::new(hosts, named.method_hosts);

    let ctor = rt::supervisor::Ctor {
        shared_data,
        modules,
        locked_slots,
        leader_nondet_results: named.leader_nondet_results,
        multi_host,
        record_actions: named.record_actions,
    };

    Ok(ExecutionContext {
        supervisor: rt::supervisor::Supervisor::start(config, ctor)?,
        det_limiter: limiter_det,
    })
}

fn convert_message_data(
    message: genvm_modules_interfaces::MessageData,
) -> genlayer_sdk::abi::entry::MessageData {
    genlayer_sdk::abi::entry::MessageData {
        contract_address: message.contract_address,
        sender_address: message.sender_address,
        origin_address: message.origin_address,
        signer_address: message.signer_address,
        stack: Vec::new(),
        chain_id: message.chain_id,
        value: message.value,
        is_init: message.is_init,
        datetime: message.datetime,
    }
}

fn extra_leader_output_error(
    supervisor: &rt::supervisor::Supervisor,
    run_ok: &rt::vm::RunOk,
) -> Option<public_abi::VmError> {
    if supervisor.shared_data.run_mode == rt::RunMode::Leader {
        return None;
    }

    let executed = supervisor
        .nondet_call_no
        .load(std::sync::atomic::Ordering::SeqCst);
    let published = supervisor
        .leader_nondet_results
        .as_ref()
        .map_or(0, |v| v.len() as u32);

    if published <= executed {
        return None;
    }

    Some(rt::errors::vm_error_for_leader_extra(&run_ok.as_bytes()))
}

pub async fn run_with_impl(
    entry_data: genvm_modules_interfaces::ExecutionData,
    context: &ExecutionContext,
    permissions: &str,
    deploy_pin: &mut Option<runners::cache::ArchivePin>,
) -> anyhow::Result<rt::vm::FullResult> {
    let supervisor = context.supervisor.clone();
    let storage_pages_limit = supervisor.get_storage_limiter();

    let mut topmost_storage = rt::vm::storage::Storage::new(
        entry_data.message.contract_address,
        storage_pages_limit,
        wasi::genlayer_sdk::StorageHostHolder(
            supervisor.host.clone(),
            wasi::genlayer_sdk::ReadToken {
                mode: public_abi::StorageType::LatestNonFinal,
                account: entry_data.message.contract_address,
            },
        ),
    );

    let (topmost_runner_id, root_permissions) = match async {
        let entry_call_data: abi::entry::MainCallData = calldata::decode_obj(&entry_data.calldata)
            .map_err(|e| rt::errors::Error {
                kind: rt::errors::ErrorKind::Vm(
                    rt::errors::convert_vm_error_from_pending_abi(
                        public_abi_pending::VmError::malformed_entry(),
                    ),
                    None,
                ),
                context: Vec::new(),
                source: Some(Box::new(e)),
            })?;

        if entry_data.message.is_init && entry_call_data.name.is_some() {
            return Err(
                rt::errors::Error::vm(rt::errors::convert_vm_error_from_pending_abi(
                    public_abi_pending::VmError::malformed_entry(),
                ))
                .into(),
            );
        }

        // Contract-owned permissions live in the root slot, not the node-granted
        // `permissions` string; pre-read them before running the wasm.
        let root_permissions = topmost_storage.read_permissions().await?;

        let id = if let Some(code) = &entry_data.code {
            log_debug!("using provided code for execution");

            topmost_storage.write_code(code).await?;
            // Record the major this contract is deployed for, so later runs can
            // verify major compatibility (see `read_major` / major_mismatch check).
            topmost_storage
                .write_major(genvm_common::version::CURRENT.major as u8)
                .await?;

            let code_slot = rt::vm::storage::default_code_slot();
            let archive = runners::parse(code.clone()).map_err(|e| {
                rt::errors::Error::wrap(public_abi::VmError::invalid_contract().val(), e)
            })?;
            *deploy_pin = Some(supervisor.prepopulate_deploy_runner(
                entry_data.message.contract_address,
                code_slot,
                archive,
            ));

            runners::Id::Chain {
                address: entry_data.message.contract_address,
                on: runners::ChainState::Deploy,
                slot: code_slot,
            }
        } else {
            log_debug!("code is null");

            let code_slot = topmost_storage.check_major_and_resolve_code_slot().await?;

            runners::Id::Chain {
                address: entry_data.message.contract_address,
                on: runners::ChainState::Accepted,
                slot: code_slot,
            }
        };

        anyhow::Ok((id, root_permissions))
    }
    .await
    {
        Ok(v) => v,
        // A VMError raised while preparing the contract (bad code, major
        // mismatch, missing code slot) is a contract error, not an internal
        // failure: surface it as a VMError result. Genuine internal errors still
        // propagate via `?`.
        Err(e) => {
            let run_ok = rt::errors::unwrap_vm_errors(e.into())?;
            if let Some(code) = extra_leader_output_error(&supervisor, &run_ok) {
                return Ok(rt::vm::FullResult::empty_from(rt::vm::RunOk::VMError(
                    code, None,
                )));
            }
            return Ok(rt::vm::FullResult::empty_from(run_ok));
        }
    };

    let can_use_balance_for_message_fees =
        primitive_types::U256::from_little_endian(&root_permissions)
            .bit(public_abi::Permissions::CanUseBalanceForMessageFees.value() as usize);

    let data_fees_limit = supervisor.shared_data.gep(|x| &x.data_fees_limit);

    let essential_data = Box::new(wasi::genlayer_sdk::SingleVMData {
        limiter: context.det_limiter.derived(),
        depth: 0,
        spawn_kind: "initial".to_owned(),
        // Permission model: docs/website/src/spec/03-vm/02-meta-properties.rst
        conf: wasi::base::Config {
            needs_error_fingerprint: true,
            permissions: wasi::base::Permissions {
                deterministic: true,
                write_storage: permissions.contains("w"),
                send_messages: permissions.contains("s"),
                call_others: permissions.contains("c"),
                spawn_nondet: permissions.contains("n"),
                register_runners: true,
                can_use_balance_for_message_fees,
            },
            execution: wasi::base::Execution {
                state_mode: crate::public_abi::StorageType::Default,
                topmost_runner_id,
            },
        },
        message_data: ExtendedMessage {
            message: convert_message_data(entry_data.message),
            entry_kind: public_abi::EntryKind::Main,
            entry_data: entry_data.calldata,
            entry_stage_data: calldata::Value::Null,
        },
        supervisor: supervisor.clone(),

        storage: topmost_storage,
        accumulator: VMDataAccumulator {
            data_fees_limit,
            messages_value_decremented: primitive_types::U256::zero(),
            emissions: Vec::new(),
            message_fee_allocation: entry_data.message_fee_allocation,
        },
        det_subvm_hashes: Default::default(),
        // The root VM inherits no custom runners; it loads its own tree at init.
        granted_custom: Vec::new(),
    });

    let run_result = rt::spawn_apply_run(&supervisor, essential_data).await?;

    if let Some(code) = extra_leader_output_error(&supervisor, &run_result.run_ok) {
        return Ok(rt::vm::FullResult::empty_from(rt::vm::RunOk::VMError(
            code, None,
        )));
    }

    Ok(rt::vm::FullResult {
        kind: match &run_result.run_ok {
            rt::vm::RunOk::Return(_) => public_abi::ResultCode::Return,
            rt::vm::RunOk::UserError(_) => public_abi::ResultCode::UserError,
            rt::vm::RunOk::VMError(_, _) => public_abi::ResultCode::VmError,
        },
        data: match run_result.run_ok {
            rt::vm::RunOk::Return(buf) => buf,
            rt::vm::RunOk::UserError(val) => val,
            rt::vm::RunOk::VMError(msg, _) => calldata::Value::Str(msg.0.into()).into(),
        },
        backtrace: run_result.backtrace,
        wasm_store_hashes: run_result.wasm_store_hashes,
        subvm_hashes: bytes::Bytes::from(
            sha3::Digest::finalize(run_result.vm_data.det_subvm_hashes).to_vec(),
        ),
        storage_changes: run_result.vm_data.storage.make_delta(),
        emissions: run_result.vm_data.accumulator.emissions,
    })
}

pub async fn run_with(
    entry_data: genvm_modules_interfaces::ExecutionData,
    context: ExecutionContext,
    permissions: &str,
) -> anyhow::Result<host::FullResult> {
    // Deploy-time weak-cache bridge: the prepopulated deploy-runner
    // entry must outlive not just the deterministic run but also the secondary
    // nondet validator queue -- a queued nondet VM whose runner is `contract`
    // during deploy attaches to this very entry at spawn. Dropping the pin
    // earlier would make that load's outcome depend on which queue processed it.
    let mut deploy_pin: Option<runners::cache::ArchivePin> = None;

    let supervisor = context.supervisor.clone();

    let res = run_with_impl(entry_data, &context, permissions, &mut deploy_pin).await;

    log_debug!("deterministic execution done");

    let nondet_disagree_res = rt::supervisor::await_nondet_vms(&supervisor).await;

    log_debug!("non-deterministic execution done");
    drop(deploy_pin);

    let merged_result = match (res, nondet_disagree_res) {
        (Err(e_res), Err(e_nondet)) => {
            log_error!(error:ah = e_nondet; "non-deterministic execution failed");

            Err(e_res)
        }
        (Err(e_res), Ok(_)) => Err(e_res),
        (Ok(_), Err(e_nondet)) => Err(e_nondet),
        (Ok(res), Ok(c)) => Ok((res, c)),
    };

    let res = merged_result.inspect_err(|e| {
        log_error!(error:ah = &e; "internal error");
    });

    if let Ok((_, Some(disag))) = &res {
        let mut host = supervisor
            .host
            .lock_for(host::host_fns::Methods::NotifyNondetDisagreement)
            .await;
        host.notify_nondet_disagreement(*disag)
            .context("notify non-deterministic disagreement")?;
    }

    // Module (llm/web) metrics are collected by the manager from its own
    // execution context; the executor only reports its own counters here.
    let gvm_metrics = calldata::to_value(&supervisor.shared_data.metrics);

    log_info!(metrics:serde = gvm_metrics; "metrics");

    log_debug!("sending final result");

    let data_fees_remaining = supervisor.shared_data.data_fees_limit.remaining().await;
    let data_fees_consumed = supervisor.shared_data.data_fees_limit.consumed().await;
    let llm_consumption = *supervisor.shared_data.llm_consumption.lock().await;

    let res = match res {
        Ok((a, b)) => Ok(host::FullResult::new(
            a,
            supervisor.take_nondet_results().await,
            b,
            data_fees_remaining,
            data_fees_consumed,
            llm_consumption,
            supervisor.take_recorded_actions().await,
        )),
        Err(e) => Err(e),
    };

    {
        let mut host = supervisor
            .host
            .lock_for(host::host_fns::Methods::ConsumeResult)
            .await;
        host.consume_result(&res).context("consume result")?;
    }

    {
        let mut host = supervisor
            .host
            .lock_for(host::host_fns::Methods::NotifyFinished)
            .await;
        host.notify_finished().context("notify finished")?;
    }

    host::all_useful_work_done();

    res
}
