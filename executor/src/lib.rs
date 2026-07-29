pub mod caching;
pub mod config;
pub mod domain;
pub mod host;
pub mod modules;
pub mod rt;
pub mod runners;
pub mod wasi;

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
    pub memory_limit: Option<u32>,
    /// Whether this execution holds the storage-write permission, however it
    /// was granted. Slot locks bound what a run may write, so this -- not the
    /// route the run arrived by -- decides whether they are read.
    pub can_write_storage: bool,
}

pub fn create_supervisor(
    config: &config::Config,
    mut hosts: Vec<Host>,
    named: CreateSupervisorNamedArgs,
    host_data: genvm_modules_interfaces::HostData,
    shared_data: sync::DArc<rt::SharedData>,
    message: &genvm_modules_interfaces::MessageData,
) -> Result<Arc<rt::supervisor::Supervisor>> {
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

    let limiter_det =
        rt::memlimiter::Limiter::with_limit("det", named.memory_limit.unwrap_or(u32::MAX));

    let storage_host_idx =
        if (host::host_fns::Methods::StorageRead as usize) < named.method_hosts.len() {
            named.method_hosts[host::host_fns::Methods::StorageRead as usize] as usize
        } else {
            0
        };

    // Slot locks only ever reject a write, so a run that cannot write does not
    // need them -- and paying to read a set it can never consult would charge
    // memory for nothing.
    let locked_slots = if named.can_write_storage {
        hosts[storage_host_idx]
            .get_locked_slots_for_sender(
                calldata::Address::from(message.contract_address.raw()),
                calldata::Address::from(message.sender_address.raw()),
                &limiter_det,
            )
            .context("reading locked slots")?
    } else {
        host::LockedSlotsSet::empty()
    };

    let multi_host = host::MultiHost::new(hosts, named.method_hosts);

    let ctor = rt::supervisor::Ctor {
        shared_data,
        modules,
        limiter: rt::DetNondet {
            det: limiter_det,
            non_det: rt::memlimiter::Limiter::new("nondet"),
        },
        locked_slots,
        leader_nondet_results: named.leader_nondet_results,
        multi_host,
    };

    rt::supervisor::Supervisor::start(config, ctor)
}

fn convert_message_data(
    message: genvm_modules_interfaces::MessageData,
    stack: Vec<calldata::Address>,
) -> genlayer_sdk::abi::entry::MessageData {
    genlayer_sdk::abi::entry::MessageData {
        contract_address: message.contract_address,
        sender_address: message.sender_address,
        origin_address: message.origin_address,
        stack,
        chain_id: message.chain_id,
        value: message.value,
        is_init: message.is_init,
        datetime: message.datetime,
    }
}

fn convert_nested_storage_type(
    state_mode: genvm_modules_interfaces::NestedStorageType,
) -> public_abi::StorageType {
    match state_mode {
        genvm_modules_interfaces::NestedStorageType::Default => public_abi::StorageType::Default,
        genvm_modules_interfaces::NestedStorageType::LatestFinal => {
            public_abi::StorageType::LatestFinal
        }
        genvm_modules_interfaces::NestedStorageType::LatestNonFinal => {
            public_abi::StorageType::LatestNonFinal
        }
    }
}

fn convert_nested_permissions(
    permissions: genvm_modules_interfaces::NestedPermissions,
    state_mode: public_abi::StorageType,
    topmost_runner_id: runners::Id,
) -> wasi::base::Config {
    use genvm_modules_interfaces::NestedPermissions as P;

    wasi::base::Config {
        needs_error_fingerprint: true,
        is_deterministic: permissions.contains(P::DETERMINISTIC),
        can_read_storage: permissions.contains(P::READ_STORAGE),
        can_write_storage: permissions.contains(P::WRITE_STORAGE),
        can_spawn_nondet: permissions.contains(P::SPAWN_NONDET),
        can_send_messages: permissions.contains(P::SEND_MESSAGES),
        can_call_others: permissions.contains(P::CALL_OTHERS),
        can_register_runners: permissions.contains(P::REGISTER_RUNNERS),
        state_mode,
        topmost_runner_id,
    }
}

fn convert_on(on: genvm_modules_interfaces::On) -> genlayer_sdk::abi::gl_call::On {
    match on {
        genvm_modules_interfaces::On::Finalized => genlayer_sdk::abi::gl_call::On::Finalized,
        genvm_modules_interfaces::On::Accepted => genlayer_sdk::abi::gl_call::On::Accepted,
    }
}

fn convert_call_key(call_key: genvm_modules_interfaces::CallKey) -> genlayer_sdk::abi::CallKey {
    genlayer_sdk::abi::CallKey(call_key.0)
}

fn convert_internal_message_params(
    params: genvm_modules_interfaces::fees::InternalMessageParams,
) -> domain::fees::InternalMessageParams {
    domain::fees::InternalMessageParams {
        leader_timeunits_allocation: params.leader_timeunits_allocation,
        validator_timeunits_allocation: params.validator_timeunits_allocation,
        execution_budget_per_round: params.execution_budget_per_round,
        rotations: params.rotations,
        max_price_gen_per_time_unit: params.max_price_gen_per_time_unit,
        storage_fee_max_gas_price: params.storage_fee_max_gas_price,
        receipt_fee_max_gas_price: params.receipt_fee_max_gas_price,
    }
}

fn convert_external_message_params(
    params: genvm_modules_interfaces::fees::ExternalMessageParams,
) -> domain::fees::ExternalMessageParams {
    domain::fees::ExternalMessageParams {
        gas_limit: params.gas_limit,
        max_gas_price: params.max_gas_price,
    }
}

fn convert_message_allocation_node_params(
    params: genvm_modules_interfaces::fees::MessageAllocationNodeParams,
) -> domain::fees::MessageAllocationNodeParams {
    match params {
        genvm_modules_interfaces::fees::MessageAllocationNodeParams::Internal(params) => {
            domain::fees::MessageAllocationNodeParams::Internal(std::sync::Arc::new(
                convert_internal_message_params((*params).clone()),
            ))
        }
        genvm_modules_interfaces::fees::MessageAllocationNodeParams::External(params) => {
            domain::fees::MessageAllocationNodeParams::External(convert_external_message_params(
                params,
            ))
        }
    }
}

fn convert_message_allocation_node(
    node: genvm_modules_interfaces::fees::MessageAllocationNode,
) -> domain::fees::MessageAllocationNode {
    domain::fees::MessageAllocationNode {
        recipient: node.recipient,
        call_key: node.call_key.map(convert_call_key),
        budget: node.budget,
        on: convert_on(node.on),
        fee_params: convert_message_allocation_node_params(node.fee_params),
        children: node
            .children
            .into_iter()
            .map(convert_message_allocation_node)
            .collect(),
    }
}

pub async fn run_with_impl(
    mut entry_data: genvm_modules_interfaces::ExecutionData,
    supervisor: Arc<rt::supervisor::Supervisor>,
    permissions: &str,
) -> anyhow::Result<rt::vm::FullResult> {
    let storage_pages_limit = supervisor.get_storage_limiter();

    // Everything a nested run imports arrives together or not at all, so unpack
    // it once here rather than probing the same option at four call sites.
    let (imported_state_mode, imported_permissions, imported_runner_id, stack) =
        match entry_data.nested.take() {
            Some(nested) => (
                Some(convert_nested_storage_type(nested.state_mode)),
                Some(nested.permissions),
                Some(nested.topmost_runner_id),
                nested.stack,
            ),
            None => (None, None, None, Vec::new()),
        };
    let storage_read_mode = imported_state_mode.unwrap_or(public_abi::StorageType::LatestNonFinal);

    let mut topmost_storage = rt::vm::storage::Storage::new(
        entry_data.message.contract_address,
        storage_pages_limit,
        wasi::genlayer_sdk::StorageHostHolder(
            supervisor.host.clone(),
            wasi::genlayer_sdk::ReadToken {
                mode: storage_read_mode,
                account: entry_data.message.contract_address,
            },
        ),
    );

    let topmost_runner_id = match async {
        let id = if let Some(code) = &entry_data.code {
            log_debug!("using provided code for execution");

            topmost_storage.write_code(code).await?;
            // v0.2.16 has no `major` root field, so nothing else is written to
            // slot ZERO on deploy: that slot belongs to the contract's python
            // storage `Root`.

            let code_slot = rt::vm::storage::default_code_slot();
            let archive = runners::parse(code.clone()).map_err(|e| {
                rt::errors::Error::wrap(public_abi::VmError::invalid_contract().val(), e)
            })?;
            supervisor.prepopulate_deploy_runner(
                entry_data.message.contract_address,
                code_slot,
                archive,
            );

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
                // No code was supplied, so nothing seeded a deploy-state cell:
                // such an id could never be loaded from chain.
                on: runners::ChainState::for_vm(false, storage_read_mode),
                slot: code_slot,
            }
        };

        match &imported_runner_id {
            Some(imported) => {
                rt::supervisor::actions::resolve_runner_id(&supervisor, &id, &imported.0)
                    .await
                    .context("resolving imported topmost runner id")
            }
            None => Ok(id),
        }
    }
    .await
    {
        Ok(id) => id,
        // A VMError raised while preparing the contract (bad code, major
        // mismatch, missing code slot) is a contract error, not an internal
        // failure: surface it as a VMError result. Genuine internal errors still
        // propagate via `?`.
        Err(e) => {
            return Ok(rt::vm::FullResult::empty_from(
                rt::errors::unwrap_vm_errors(e.into())?,
            ));
        }
    };

    let data_fees_limit = supervisor.shared_data.gep(|x| &x.data_fees_limit);
    let conf = match imported_permissions {
        Some(imported) => convert_nested_permissions(
            imported,
            imported_state_mode.unwrap_or(public_abi::StorageType::Default),
            topmost_runner_id,
        ),
        None => wasi::base::Config {
            needs_error_fingerprint: true,
            is_deterministic: true,
            can_read_storage: true,
            can_write_storage: permissions.contains("w"),
            can_send_messages: permissions.contains("s"),
            can_call_others: permissions.contains("c"),
            can_spawn_nondet: permissions.contains("n"),
            can_register_runners: permissions.contains("u"),
            state_mode: crate::public_abi::StorageType::Default,
            topmost_runner_id,
        },
    };

    let essential_data = Box::new(wasi::genlayer_sdk::SingleVMData {
        // A budget minted elsewhere is a remainder, not an authority: a chain
        // root on a line with a looser limit must not buy depth this executor
        // does not offer.
        remaining_recursion: entry_data
            .remaining_recursion
            .map_or(public_abi::top_limits::VM_RECURSION, |supplied| {
                supplied.min(public_abi::top_limits::VM_RECURSION)
            }),
        signer_address: entry_data.message.signer_address,
        // Permission model: docs/website/src/spec/03-vm/02-meta-properties.rst
        conf,
        message_data: ExtendedMessage {
            message: convert_message_data(entry_data.message, stack),
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
            message_fee_allocation: entry_data
                .message_fee_allocation
                .into_iter()
                .map(convert_message_allocation_node)
                .collect(),
            custom_runners: Default::default(),
        },
        det_subvm_hashes: Default::default(),
    });

    let run_result = rt::spawn_apply_run(&supervisor, essential_data).await?;

    Ok(rt::vm::FullResult {
        kind: match &run_result.run_ok {
            rt::vm::RunOk::Return(_) => public_abi::ResultCode::Return,
            rt::vm::RunOk::UserError(_) => public_abi::ResultCode::UserError,
            rt::vm::RunOk::VMError(_, _) => public_abi::ResultCode::VmError,
        },
        data: match run_result.run_ok {
            rt::vm::RunOk::Return(buf) => buf,
            rt::vm::RunOk::UserError(msg) => calldata::Value::Str(msg).into(),
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
    supervisor: Arc<rt::supervisor::Supervisor>,
    permissions: &str,
) -> anyhow::Result<host::FullResult> {
    let res = run_with_impl(entry_data, supervisor.clone(), permissions).await;

    log_debug!("deterministic execution done");

    let nondet_disagree_res = rt::supervisor::await_nondet_vms(&supervisor).await;

    log_debug!("non-deterministic execution done");

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

    supervisor
        .host
        .flush_all()
        .await
        .context("flush hosts before exit")?;

    host::all_useful_work_done();

    res
}
