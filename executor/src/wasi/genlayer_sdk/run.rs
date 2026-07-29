use super::*;
use crate::runners;
use sha3::digest::Update;
use std::sync::Arc;

async fn consume_nondet_output(
    shared_data: &rt::SharedData,
    output_length: u64,
) -> Result<(), generated::types::Error> {
    if !shared_data
        .data_fees_limit
        .consume_nondet_output(output_length)
        .await
        .map_err(internal_trap)?
    {
        return Err(internal_trap(rt::errors::Error::vm(
            abi::consts::VmError::out_of().receipt().nondet_output(),
        )));
    }
    Ok(())
}

/// Is this leader-proposed `vm_error` code acceptable as-is?
///
/// `Err` carries the code a validator derives instead -- either
/// `leader_output malformed` or, for a proposal that reaches into the
/// derived-outcome namespace, `leader_output uses_this_error <h>`.
fn validate_leader_vm_error(code: &str) -> Result<(), public_abi::VmError> {
    // An honest leader strips its own detail before publishing, so a proposal
    // carrying one is malformed rather than something to strip.
    if code.contains(" # ") {
        return Err(malformed_leader_result());
    }

    // The derived-outcome namespace is checked *before* validity, so proposing
    // the literal `leader_output malformed` cannot yield an output byte-equal
    // to the proposal.
    if rt::errors::vm_error_is_of_kind(code, public_abi_pending::VmError::leader_output().prefix_())
        || rt::errors::vm_error_is_of_kind(
            code,
            &abi::consts::VmError::absent_leader_nondet_output().0,
        )
    {
        return Err(rt::errors::vm_error_for_leader_use_this_error(code));
    }

    // Only `vm_error` trie paths are proposable. The pending-ABI codes are
    // deliberately excluded: `malformed_entry` is an outcome the executor
    // derives, never one a leader may claim.
    if !public_abi::VmError::is_valid_(code) {
        return Err(malformed_leader_result());
    }

    Ok(())
}

fn malformed_leader_result() -> public_abi::VmError {
    rt::errors::convert_vm_error_from_pending_abi(
        public_abi_pending::VmError::leader_output().malformed(),
    )
}

/// The leader's own `VMError` code as it is published to the host. The fused
/// ` # <detail>` suffix is dropped -- the non-deterministic result channel is
/// detail-free -- and *nothing else* happens: running the leader-result
/// acceptance check over an honest result could only rewrite it into a
/// derived-namespace code that validators replace again, which is a guaranteed
/// hash mismatch between honest nodes.
///
/// The remaining validity is by construction (codes come from the generated
/// constructors and from canonical `exit_code <i32>`); the assertion is a
/// codegen-drift tripwire, not a runtime check.
pub fn strip_vm_error_detail(code: &str) -> public_abi::VmError {
    let public_code = code.split_once(" # ").map_or(code, |(c, _)| c);

    debug_assert!(
        public_abi::VmError::is_valid_(public_code),
        "leader computed a vm_error outside the trie: {public_code:?}"
    );

    public_abi::VmError(std::borrow::Cow::Owned(public_code.to_owned()))
}

/// The one total parse of a leader-proposed non-deterministic result. `Ok`
/// means the bytes are accepted verbatim (`as_bytes()` reproduces `data`);
/// `Err` carries the VM error the validator derives instead. Malformed input
/// never traps and never bypasses the comparison stage.
pub fn parse_leader_result(data: &[u8]) -> Result<rt::vm::RunOk, public_abi::VmError> {
    let Some((&code, rest)) = data.split_first() else {
        return Err(public_abi::VmError::absent_leader_nondet_output());
    };

    let code = public_abi::ResultCode::try_from(code).map_err(|()| malformed_leader_result())?;

    match code {
        public_abi::ResultCode::Return => {
            // Decoding yields a byte-faithful `Maybe::Checked(Raw(..))`, so keep it
            // directly instead of re-copying `rest` into a fresh `Raw`.
            let ret: calldata::unparsed::Maybe<calldata::Value> =
                calldata::decode_obj(rest).map_err(|_| malformed_leader_result())?;

            Ok(rt::vm::RunOk::Return(ret))
        }
        public_abi::ResultCode::UserError => {
            let err: calldata::unparsed::Maybe<calldata::Value> =
                calldata::decode_obj(rest).map_err(|_| malformed_leader_result())?;

            Ok(rt::vm::RunOk::UserError(err))
        }
        public_abi::ResultCode::VmError => {
            let code = std::str::from_utf8(rest).map_err(|_| malformed_leader_result())?;

            validate_leader_vm_error(code)?;

            Ok(rt::vm::RunOk::VMError(
                public_abi::VmError(std::borrow::Cow::Owned(code.to_owned())),
                None,
            ))
        }

        public_abi::ResultCode::InternalError => Err(malformed_leader_result()),
    }
}

struct RunNondetGetVMTaskArgs {
    child_topmost_id: runners::Id,
    storage_checkpoint: rt::vm::storage::Storage<wasi::genlayer_sdk::StorageHostHolder>,
    child_custom: Vec<runners::cache::ArchivePin>,
    call_no: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum CallContractRoute {
    InProcess,
    Nested(bytes::Bytes),
}

pub(super) fn call_contract_route(
    routing_payload: Option<bytes::Bytes>,
    advisory_major: u8,
) -> CallContractRoute {
    if let Some(payload) = routing_payload {
        return CallContractRoute::Nested(payload);
    }
    // The host declined to place the callee. A major this line does not serve
    // is still not ours to reject: the manager owns the mapping from a major to
    // an executor line, so hand the call over and let it answer.
    if rt::vm::storage::Storage::<StorageHostHolder>::check_major(advisory_major).is_err() {
        let payload =
            calldata::encode_obj(&genvm_modules_interfaces::ExecutorSelector::MajorOverride {
                major: advisory_major as u32,
            });
        return CallContractRoute::Nested(payload.into());
    }
    CallContractRoute::InProcess
}

pub(super) fn derive_call_contract_permissions(parent: &base::Permissions) -> base::Permissions {
    base::Permissions {
        deterministic: true,
        write_storage: false,
        spawn_nondet: false,
        call_others: parent.call_others,
        send_messages: false,
        register_runners: parent.register_runners,
        can_use_balance_for_message_fees: false,
    }
}

fn cross_major_internal(operation: &str, error: impl std::fmt::Display) -> generated::types::Error {
    generated::types::Error::trap(crate::anyhow_to_wasmtime(
        rt::errors::Error::internal(format!("{operation}: {error}")).into(),
    ))
}

pub(super) fn nested_run_ok(
    reply: genvm_modules_interfaces::NestedRunReply,
) -> anyhow::Result<(rt::vm::RunOk, bytes::Bytes)> {
    anyhow::ensure!(
        reply.effect_free,
        "nested CallContract result contains effects"
    );
    anyhow::ensure!(
        reply.small_hash.len() == 32,
        "nested CallContract small hash has length {}, expected 32",
        reply.small_hash.len()
    );

    let run_ok = match reply.result.kind {
        genvm_modules_interfaces::ResultCode::Return => rt::vm::RunOk::Return(reply.result.data),
        genvm_modules_interfaces::ResultCode::UserError => {
            rt::vm::RunOk::UserError(reply.result.data)
        }
        genvm_modules_interfaces::ResultCode::VmError => {
            let data = reply.result.data.materialize()?;
            let calldata::Value::Str(code) = data else {
                anyhow::bail!("nested CallContract VM error is not a string");
            };
            rt::vm::RunOk::VMError(public_abi::VmError(std::borrow::Cow::Owned(code)), None)
        }
        genvm_modules_interfaces::ResultCode::InternalError => {
            anyhow::bail!("nested executor returned an internal error");
        }
    };

    Ok((run_ok, reply.small_hash))
}

impl ContextVFS<'_> {
    pub(super) async fn gl_call_eth_call(
        &mut self,
        address: calldata::Address,
        calldata: bytes::Bytes,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        if !self.context.data.conf.permissions.deterministic {
            return Err(generated::types::Errno::Forbidden.into());
        }
        if !self.context.data.conf.permissions.call_others {
            return Err(generated::types::Errno::Forbidden.into());
        }

        let supervisor = self.context.data.supervisor.clone();
        let data = supervisor
            .host
            .lock_for(host::host_fns::Methods::EthCall)
            .await
            .eth_call(address, &calldata)
            .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;
        self.place_content(vfs::FileContents::from(bytes::Bytes::from(data)))
    }

    fn run_nondet_get_vm_task(
        &mut self,
        message_data: ExtendedMessage,
        args: RunNondetGetVMTaskArgs,
    ) -> rt::supervisor::NonDetVMTask {
        let fake_accum = VMDataAccumulator {
            data_fees_limit: self.context.data.accumulator.data_fees_limit.clone(),
            messages_value_decremented: self.context.data.accumulator.messages_value_decremented,
            emissions: Vec::new(),
            message_fee_allocation: Vec::new(),
        };

        let vm_data = Box::new(SingleVMData {
            limiter: Default::default(),
            remaining_recursion: self.context.data.remaining_recursion.saturating_sub(1),
            spawn_kind: "run_nondet".to_owned(),
            // Permission model: docs/website/src/spec/03-vm/02-meta-properties.rst
            conf: base::Config {
                needs_error_fingerprint: false,
                permissions: base::Permissions {
                    deterministic: false,
                    write_storage: false,
                    send_messages: false,
                    call_others: false,
                    spawn_nondet: false,
                    register_runners: false,
                    can_use_balance_for_message_fees: false,
                },
                execution: base::Execution {
                    state_mode: public_abi::StorageType::Default,
                    topmost_runner_id: args.child_topmost_id,
                },
            },
            message_data,
            supervisor: self.context.data.supervisor.clone(),
            storage: args.storage_checkpoint,
            accumulator: fake_accum,
            det_subvm_hashes: Default::default(), // won't be used
            // The nondet child is granted exactly the parent-computed set; the
            // pins keep the content alive until the (possibly queued) child
            // spawns and load-actions them against its own limiter.
            granted_custom: args.child_custom,
        });

        rt::supervisor::NonDetVMTask {
            task: vm_data,
            call_no: args.call_no,
            tasks_done: Arc::new(tokio::sync::Notify::new()),
        }
    }

    fn derive_call_contract_vm_data(
        &self,
        address: calldata::Address,
        calldata: &abi::entry::MainCallData,
        state: public_abi::StorageType,
        code_slot: SlotID,
        child_storage: rt::vm::storage::Storage<StorageHostHolder>,
    ) -> Box<SingleVMData> {
        let supervisor = self.context.data.supervisor.clone();
        let my_conf = self.context.data.conf.clone();
        let mut my_data = self.context.data.message_data.fork(
            public_abi::EntryKind::Main,
            calldata::encode_obj(calldata).into(),
        );
        my_data.message.stack.push(my_data.message.contract_address);

        Box::new(SingleVMData {
            limiter: self.context.limiter.derived(),
            remaining_recursion: self.context.data.remaining_recursion.saturating_sub(1),
            spawn_kind: "call_contract".to_owned(),
            // Permission model: docs/website/src/spec/03-vm/02-meta-properties.rst
            conf: base::Config {
                needs_error_fingerprint: true,
                permissions: derive_call_contract_permissions(&my_conf.permissions),
                execution: base::Execution {
                    state_mode: state,
                    topmost_runner_id: runners::Id::Chain {
                        address,
                        on: if state == public_abi::StorageType::LatestFinal {
                            runners::ChainState::Finalized
                        } else {
                            runners::ChainState::Accepted
                        },
                        slot: code_slot,
                    },
                },
            },
            message_data: ExtendedMessage {
                message: genlayer_sdk::abi::entry::MessageData {
                    contract_address: address,
                    sender_address: my_data.message.sender_address,
                    origin_address: my_data.message.origin_address,
                    signer_address: my_data.message.signer_address,
                    value: num_bigint::BigInt::ZERO,
                    is_init: false,
                    datetime: my_data.message.datetime,
                    chain_id: my_data.message.chain_id,
                    stack: my_data.message.stack,
                },
                entry_kind: my_data.entry_kind,
                entry_data: my_data.entry_data,
                entry_stage_data: default_entry_stage_data(),
            },
            storage: child_storage,
            supervisor,
            accumulator: VMDataAccumulator {
                data_fees_limit: self.context.data.accumulator.data_fees_limit.clone(),
                messages_value_decremented: primitive_types::U256::zero(),
                emissions: Vec::new(),
                message_fee_allocation: Vec::new(),
            },
            det_subvm_hashes: Default::default(),
            // A CallContract child is granted the caller's full custom set;
            // its spawn load-actions each into the child.
            granted_custom: self.context.loaded.custom_pins(),
        })
    }

    async fn run_nested_call_contract(
        &mut self,
        routing_payload: bytes::Bytes,
        vm_data: Box<SingleVMData>,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        use genvm_modules_interfaces::{
            NestedPermissions as P, NestedRunEnvelope, NestedRunnerId, NestedStorageType,
        };

        let host_remaining_fuel = vm_data
            .supervisor
            .host
            .lock_for(host::host_fns::Methods::RemainingFuelAsGen)
            .await
            .remaining_fuel_as_gen()
            .map_err(|e| cross_major_internal("reading nested deterministic fuel", e))?;
        let remaining_det_fuel = vm_data
            .supervisor
            .shared_data
            .remaining_det_fuel(host_remaining_fuel)
            .await;

        let mut permissions = P::READ_STORAGE;
        if vm_data.conf.permissions.deterministic {
            permissions |= P::DETERMINISTIC;
        }
        if vm_data.conf.permissions.write_storage {
            permissions |= P::WRITE_STORAGE;
        }
        if vm_data.conf.permissions.send_messages {
            permissions |= P::SEND_MESSAGES;
        }
        if vm_data.conf.permissions.call_others {
            permissions |= P::CALL_OTHERS;
        }
        if vm_data.conf.permissions.spawn_nondet {
            permissions |= P::SPAWN_NONDET;
        }
        if vm_data.conf.permissions.register_runners {
            permissions |= P::REGISTER_RUNNERS;
        }
        if vm_data.conf.permissions.can_use_balance_for_message_fees {
            permissions |= P::USE_BALANCE_FOR_MESSAGE_FEES;
        }

        let state_mode = match vm_data.conf.execution.state_mode {
            public_abi::StorageType::Default => NestedStorageType::Default,
            public_abi::StorageType::LatestFinal => NestedStorageType::LatestFinal,
            public_abi::StorageType::LatestNonFinal => NestedStorageType::LatestNonFinal,
        };
        let message = &vm_data.message_data.message;
        let envelope = NestedRunEnvelope {
            routing_payload,
            calldata: vm_data.message_data.entry_data.clone(),
            message: genvm_modules_interfaces::MessageData {
                contract_address: message.contract_address,
                sender_address: message.sender_address,
                origin_address: message.origin_address,
                signer_address: message.signer_address,
                chain_id: message.chain_id.clone(),
                value: message.value.clone(),
                is_init: message.is_init,
                datetime: message.datetime,
            },
            stack: message.stack.clone(),
            permissions,
            state_mode,
            topmost_runner_id: NestedRunnerId("contract".to_owned()),
            remaining_recursion: vm_data.remaining_recursion,
            remaining_det_fuel,
            memory_limit: vm_data.limiter.get_remaining_memory(),
        };

        let reply = vm_data
            .supervisor
            .host
            .lock_for(host::host_fns::Methods::RunNested)
            .await
            .run_nested(&envelope)
            .map_err(|e| cross_major_internal("running nested executor", e))?;
        let (run_ok, small_hash) =
            nested_run_ok(reply).map_err(|e| cross_major_internal("reading nested result", e))?;

        self.context.data.det_subvm_hashes.update(&small_hash);
        self.set_vm_run_result(run_ok).map(|x| x.0)
    }

    pub(super) async fn gl_call_contract(
        &mut self,
        address: calldata::Address,
        calldata: abi::entry::MainCallData,
        mut state: public_abi::StorageType,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        if !self.context.data.conf.permissions.deterministic {
            return Err(generated::types::Errno::Forbidden.into());
        }
        if !self.context.data.conf.permissions.call_others {
            return Err(generated::types::Errno::Forbidden.into());
        }

        let supervisor = self.context.data.supervisor.clone();

        if state == public_abi::StorageType::Default {
            state = self.context.data.conf.execution.state_mode;
        }

        let mut child_storage = rt::vm::storage::Storage::new(
            address,
            supervisor.get_storage_limiter(),
            StorageHostHolder(
                supervisor.host.clone(),
                ReadToken {
                    account: address,
                    mode: state,
                },
            ),
        );

        let (advisory_major, code_slot) = child_storage
            .read_major_and_resolve_code_slot()
            .await
            .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e.into())))?;
        let routing_payload = supervisor
            .host
            .lock_for(host::host_fns::Methods::ResolveCallcontractExecutor)
            .await
            .resolve_callcontract_executor(address, state, advisory_major)
            .map_err(|e| cross_major_internal("resolving CallContract executor", e))?;
        let route = call_contract_route(routing_payload, advisory_major);
        let vm_data =
            self.derive_call_contract_vm_data(address, &calldata, state, code_slot, child_storage);

        match route {
            CallContractRoute::Nested(routing_payload) => {
                if vm_data.remaining_recursion == 0 {
                    return Err(internal_trap(rt::errors::Error::vm(
                        public_abi::VmError::out_of().vm_recursion(),
                    )));
                }
                // Custom runners are process-local: the envelope carries no
                // archives, so a callee in another executor could not load them
                // and would silently run with an empty set. Refuse the call
                // instead, so the caller sees a canonical error rather than a
                // child that resolves `custom:` ids differently per route.
                if !vm_data.granted_custom.is_empty() {
                    return Err(generated::types::Errno::Inval.into());
                }
                self.run_nested_call_contract(routing_payload, vm_data)
                    .await
            }
            CallContractRoute::InProcess => {
                rt::vm::storage::Storage::<StorageHostHolder>::check_major(advisory_major)
                    .map_err(|e| {
                        generated::types::Error::trap(crate::anyhow_to_wasmtime(e.into()))
                    })?;

                let res = spawn_sub_vm(supervisor, vm_data)
                    .await
                    .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

                // The child is read-only (static), so its accumulator must be
                // empty -- otherwise an effect was charged but discarded here.
                res.vm_data
                    .accumulator
                    .check_empty()
                    .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

                let hash = res.small_hash();
                self.context.data.det_subvm_hashes.update(&hash);

                self.set_vm_run_result(res.run_ok).map(|x| x.0)
            }
        }
    }

    pub(super) async fn run_nondet(
        &mut self,
        data_leader: bytes::Bytes,
        data_validator: bytes::Bytes,
        runner: Option<String>,
        custom_runners: Option<Vec<String>>,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        if !self.context.data.conf.permissions.spawn_nondet {
            return Err(generated::types::Errno::Forbidden.into());
        }

        // Resolve the runner to execute and the child's custom-runner visibility
        // in this (deterministic parent) scope, so malformed inputs fail
        // deterministically at gl_call time.
        let supervisor = self.context.data.supervisor.clone();
        let parent_runner_id = self.context.data.conf.execution.topmost_runner_id.clone();
        let child_topmost_id = match &runner {
            None => parent_runner_id.clone(),
            Some(r) => {
                rt::supervisor::actions::resolve_runner_id(&supervisor, &parent_runner_id, r)
                    .await
                    .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?
            }
        };
        let child_custom = rt::supervisor::actions::resolve_child_custom_runners(
            &self.context.loaded,
            custom_runners,
            &child_topmost_id,
        )
        .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

        let call_no = self
            .context
            .data
            .supervisor
            .nondet_call_no
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        if call_no >= public_abi::top_limits::NONDET_BLOCKS {
            return Err(generated::types::Error::trap(crate::anyhow_to_wasmtime(
                rt::errors::Error::vm(abi::consts::VmError::out_of().nondet_blocks()).into(),
            )));
        }

        let run_nondet_get_vm_task_args = RunNondetGetVMTaskArgs {
            child_topmost_id,
            storage_checkpoint: self.context.data.storage.clone(),
            child_custom,
            call_no,
        };

        let result_to_return = if self.context.data.supervisor.shared_data.run_mode
            == rt::RunMode::Leader
        {
            let vm_ext_msg = self.context.data.message_data.fork_leader(
                public_abi::EntryKind::ConsensusStage,
                data_leader,
                None,
            );

            let task = self.run_nondet_get_vm_task(vm_ext_msg, run_nondet_get_vm_task_args);

            let computed_result = task
                .run_now(&self.context.data.supervisor)
                .await
                .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

            let computed_result = match computed_result {
                // Publish the bare code, keep the detail as a local cause.
                rt::vm::RunOk::VMError(err, cause) => {
                    rt::vm::RunOk::VMError(strip_vm_error_detail(&err.0), cause)
                }
                computed_result => computed_result,
            };

            self.context
                .data
                .supervisor
                .push_nondet_result(call_no, bytes::Bytes::from(computed_result.as_bytes()))
                .await;

            computed_result
        } else {
            let leaders_res_bytes = self
                .context
                .data
                .supervisor
                .get_leader_nondet_result(call_no);

            // Absent and empty must produce the same outcome (`Supervisor`
            // pads gaps with empty `Bytes`), which is exactly what the empty
            // slice already yields -- so one call covers both.
            let leaders_res = match parse_leader_result(&leaders_res_bytes.unwrap_or_default()) {
                Ok(res) => res,
                Err(vm_error) => rt::vm::RunOk::VMError(vm_error, None),
            };

            if self.context.data.supervisor.shared_data.run_mode == rt::RunMode::Validator {
                let vm_ext_msg = self.context.data.message_data.fork_leader(
                    public_abi::EntryKind::ConsensusStage,
                    data_validator,
                    Some(leaders_res.duplicate()),
                );

                let task = self.run_nondet_get_vm_task(vm_ext_msg, run_nondet_get_vm_task_args);

                rt::supervisor::submit_nondet_vm_task(&self.context.data.supervisor, task).await;
            }

            leaders_res
        };

        consume_nondet_output(
            &self.context.data.supervisor.shared_data,
            result_to_return.as_bytes().len() as u64,
        )
        .await?;

        self.set_vm_run_result(result_to_return).map(|x| x.0)
    }

    pub(super) async fn sandbox(
        &mut self,
        data: bytes::Bytes,
        runner: String,
        allow_write_storage: bool,
        allow_send_messages: bool,
        allow_register_runners: bool,
        custom_runners: Option<Vec<String>>,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        let supervisor = self.context.data.supervisor.clone();

        let parent_runner_id = self.context.data.conf.execution.topmost_runner_id.clone();
        let topmost_runner_id =
            rt::supervisor::actions::resolve_runner_id(&supervisor, &parent_runner_id, &runner)
                .await
                .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

        // Grants are validated against, and drawn from, this VM's loaded set.
        // No flow-back is possible by construction: the child's own loaded set (and
        // any runner it registers) dies with it, and the parent's loaded set is
        // never mutated here.
        let child_custom = rt::supervisor::actions::resolve_child_custom_runners(
            &self.context.loaded,
            custom_runners,
            &topmost_runner_id,
        )
        .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

        let message_data = self
            .context
            .data
            .message_data
            .fork(public_abi::EntryKind::Sandbox, data);

        let zelf_conf = &self.context.data.conf;

        let storage_checkpoint = self.context.data.storage.clone();

        let mut fake_my_data = VMDataAccumulator {
            data_fees_limit: self.context.data.accumulator.data_fees_limit.clone(),
            messages_value_decremented: primitive_types::U256::max_value(),
            emissions: Vec::new(),
            message_fee_allocation: Vec::new(),
        };

        std::mem::swap(&mut self.context.data.accumulator, &mut fake_my_data);

        let stolen_data = fake_my_data;

        let vm_data = Box::new(SingleVMData {
            limiter: self.context.limiter.derived(),
            remaining_recursion: self.context.data.remaining_recursion.saturating_sub(1),
            spawn_kind: "sandbox".to_owned(),
            // Permission model: docs/website/src/spec/03-vm/02-meta-properties.rst
            conf: base::Config {
                needs_error_fingerprint: false,
                permissions: base::Permissions {
                    deterministic: zelf_conf.permissions.deterministic,
                    write_storage: zelf_conf.permissions.write_storage & allow_write_storage,
                    send_messages: zelf_conf.permissions.send_messages & allow_send_messages,
                    call_others: false,
                    spawn_nondet: false,
                    register_runners: zelf_conf.permissions.register_runners
                        & allow_register_runners,
                    can_use_balance_for_message_fees: zelf_conf
                        .permissions
                        .can_use_balance_for_message_fees
                        & allow_send_messages,
                },
                execution: base::Execution {
                    state_mode: zelf_conf.execution.state_mode,
                    topmost_runner_id,
                },
            },
            message_data,
            supervisor: supervisor.clone(),
            storage: storage_checkpoint,
            accumulator: stolen_data,
            det_subvm_hashes: Default::default(),
            granted_custom: child_custom,
        });

        let my_res = spawn_sub_vm(supervisor.clone(), vm_data)
            .await
            .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

        if self.context.data.conf.permissions.deterministic {
            let hash = my_res.small_hash();
            self.context.data.det_subvm_hashes.update(&hash);
        }

        self.context.data.accumulator = my_res.vm_data.accumulator;
        self.context.data.storage = my_res.vm_data.storage;

        let data: Vec<u8> = my_res.run_ok.as_bytes();
        self.place_content(vfs::FileContents::from(bytes::Bytes::from(data)))
    }
}
