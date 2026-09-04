use super::*;
use crate::runners;
use sha3::digest::Update;
use std::sync::Arc;

async fn consume_preflighted_nondet_output(fees: &rt::fees::DataLimit, output_length: u64) {
    let consumed = fees
        .consume_nondet_output(output_length)
        .await
        .expect("preflighted nondeterministic output fee evaluation must succeed");
    assert!(
        consumed,
        "preflighted nondeterministic output fee must remain available"
    );
}

async fn can_consume_nondet_output(
    fees: &rt::fees::DataLimit,
    output_length: u64,
) -> Result<bool, generated::types::Error> {
    fees.can_consume_nondet_output(output_length)
        .await
        .map_err(internal_trap)
}

pub(super) struct NondetOutput {
    pub(super) result: rt::vm::RunOk,
    pub(super) encoded: rt::vm::ContractResultBytes,
}

impl NondetOutput {
    pub(super) fn from_outcome(outcome: rt::vm::ContractOutcome) -> Self {
        Self {
            result: outcome.duplicate().into(),
            encoded: outcome.encode(),
        }
    }

    pub(super) fn vm_error(error: public_abi::VmError) -> Self {
        Self::from_outcome(rt::vm::ContractOutcome::VMError(error, None))
    }

    fn duplicate(&self) -> Self {
        Self {
            result: self.result.duplicate(),
            encoded: self.encoded.clone(),
        }
    }

    fn is_fatal(&self) -> bool {
        matches!(&self.result, rt::vm::RunOk::FatalVMError(..))
    }

    fn duplicate_preserving_fatality_of(&self, source: &Self) -> Self {
        let mut output = self.duplicate();
        if source.is_fatal() {
            let rt::vm::RunOk::VMError(error, _) = output.result else {
                unreachable!("canonical nondeterministic errors are non-fatal")
            };
            output.result = rt::vm::RunOk::FatalVMError(error, None);
        }
        output
    }

    pub(super) fn allocation_size(&self) -> u64 {
        usize_into_u64(self.encoded.as_slice().len())
            .saturating_add(memory_limiter_consts::NONDET_OUTPUT_BASE_SIZE.into())
    }

    pub(super) fn preflight_ram_size(&self) -> u64 {
        self.allocation_size()
            .saturating_add(usize_into_u64(self.encoded.as_slice().len()))
            .saturating_add(memory_limiter_consts::FD_ALLOCATION.into())
    }
}

pub(super) fn reserve_nondet_output(
    limiter: &rt::memlimiter::Limiter,
    output: NondetOutput,
    memory_error: &NondetOutput,
) -> Result<(NondetOutput, rt::memlimiter::PermanentAllocation), generated::types::Error> {
    match limiter.reserve_permanent(output.allocation_size()) {
        Some(allocation) => Ok((output, allocation)),
        None => {
            let output = memory_error.duplicate_preserving_fatality_of(&output);
            let allocation = reserve_permanent(
                limiter,
                output.allocation_size(),
                "nondeterministic memory error",
            )?;
            Ok((output, allocation))
        }
    }
}

pub(super) fn preflight_nondet_output_ram(
    limiter: &rt::memlimiter::Limiter,
    memory_error: &NondetOutput,
    fee_error: &NondetOutput,
) -> Result<(), generated::types::Error> {
    let fallback_size = memory_error
        .preflight_ram_size()
        .max(fee_error.preflight_ram_size());
    drop(reserve_permanent(
        limiter,
        fallback_size,
        "nondeterministic fallback output",
    )?);
    Ok(())
}

pub(super) async fn preflight_nondet_output_fees(
    fees: &rt::fees::DataLimit,
    memory_error: &NondetOutput,
    fee_error: &NondetOutput,
) -> Result<(), generated::types::Error> {
    for error in [memory_error, fee_error] {
        if !can_consume_nondet_output(fees, error.encoded.as_slice().len().into_int_comptime())
            .await?
        {
            return Err(internal_trap(rt::errors::Error::vm(
                abi::consts::VmError::out_of().receipt().nondet_output(),
            )));
        }
    }
    Ok(())
}

pub(super) async fn charge_nondet_output(
    limiter: &rt::memlimiter::Limiter,
    fees: &rt::fees::DataLimit,
    output: NondetOutput,
    memory_error: &NondetOutput,
    fee_error: &NondetOutput,
) -> Result<NondetOutput, generated::types::Error> {
    let mut output = output;
    if !can_consume_nondet_output(fees, output.encoded.as_slice().len().into_int_comptime()).await?
    {
        output = fee_error.duplicate_preserving_fatality_of(&output);
    }

    let (output, allocation) = reserve_nondet_output(limiter, output, memory_error)?;
    consume_preflighted_nondet_output(fees, output.encoded.as_slice().len().into_int_comptime())
        .await;
    allocation.commit();
    Ok(output)
}

/// Is this leader-proposed `vm_error` code acceptable as-is?
///
/// `Err` carries the code a validator derives instead -- either
/// `leader_fault nondet_output malformed` or, for a proposal that reaches into
/// the derived-outcome namespace,
/// `leader_fault nondet_output uses_this_error <h>`.
fn validate_leader_vm_error(code: &str) -> Result<(), public_abi::VmError> {
    // An honest leader strips its own detail before publishing, so a proposal
    // carrying one is malformed rather than something to strip.
    if code.contains(" # ") {
        return Err(malformed_leader_result());
    }

    // The leader-fault nondet-output subtree is derived by validators, never
    // proposable. It is checked *before* validity, so proposing the literal
    // `leader_fault nondet_output malformed` cannot yield an output byte-equal
    // to the proposal.
    if rt::errors::vm_error_is_of_kind(
        code,
        public_abi::VmError::leader_fault()
            .nondet_output()
            .prefix_(),
    ) {
        return Err(rt::errors::vm_error_for_leader_use_this_error(code));
    }

    // Only `vm_error` trie paths are proposable. Derived leader-result codes are
    // rejected above, and `malformed_entry` is an outcome the executor derives.
    if !public_abi::VmError::is_valid_(code) {
        return Err(malformed_leader_result());
    }

    if code == public_abi::VmError::malformed_entry().0.as_ref() {
        return Err(malformed_leader_result());
    }

    Ok(())
}

fn malformed_leader_result() -> public_abi::VmError {
    public_abi::VmError::leader_fault()
        .nondet_output()
        .malformed()
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
/// means the bytes are accepted verbatim (`encode()` reproduces `data`);
/// `Err` carries the VM error the validator derives instead. Malformed input
/// never traps and never bypasses the comparison stage.
pub fn parse_leader_result(data: &[u8]) -> Result<rt::vm::ContractOutcome, public_abi::VmError> {
    let Some((&code, rest)) = data.split_first() else {
        return Err(public_abi::VmError::leader_fault().nondet_output().absent());
    };

    let code = public_abi::ResultCode::try_from(code).map_err(|()| malformed_leader_result())?;

    match code {
        public_abi::ResultCode::Return => {
            // Decoding yields a byte-faithful `Maybe::Checked(Raw(..))`, so keep it
            // directly instead of re-copying `rest` into a fresh `Raw`.
            let ret: calldata::unparsed::Maybe<calldata::Value> =
                calldata::decode_obj(rest).map_err(|_| malformed_leader_result())?;

            Ok(rt::vm::ContractOutcome::Return(ret))
        }
        public_abi::ResultCode::UserError => {
            let err: calldata::unparsed::Maybe<calldata::Value> =
                calldata::decode_obj(rest).map_err(|_| malformed_leader_result())?;

            Ok(rt::vm::ContractOutcome::UserError(err))
        }
        public_abi::ResultCode::VmError => {
            let code = std::str::from_utf8(rest).map_err(|_| malformed_leader_result())?;

            validate_leader_vm_error(code)?;

            Ok(rt::vm::ContractOutcome::VMError(
                public_abi::VmError(std::borrow::Cow::Owned(code.to_owned())),
                None,
            ))
        }
    }
}

pub(super) fn leader_outcome_for_publication(
    computed_result: rt::vm::RunOk,
) -> rt::errors::Result<rt::vm::ContractOutcome> {
    match rt::vm::ContractOutcome::try_from(computed_result)? {
        // Publish the bare code, keep the detail as a local cause
        rt::vm::ContractOutcome::VMError(err, cause) => Ok(rt::vm::ContractOutcome::VMError(
            strip_vm_error_detail(&err.0),
            cause,
        )),
        computed_result => Ok(computed_result),
    }
}

/// What a validator makes of the leader's proposal for one nondet block
pub(super) enum LeaderProposal {
    Accepted(rt::vm::ContractOutcome),
    /// No honest leader could have produced these bytes
    Rejected(public_abi::VmError),
}

impl LeaderProposal {
    /// The outcome the caller receives and the bytes the block is charged for
    pub fn into_result_and_encoding(self) -> (rt::vm::RunOk, rt::vm::ContractResultBytes) {
        match self {
            Self::Accepted(outcome) => (outcome.duplicate().into(), outcome.encode()),
            Self::Rejected(vm_error) => (
                rt::vm::RunOk::FatalVMError(vm_error.clone(), None),
                rt::vm::ContractOutcome::VMError(vm_error, None).encode(),
            ),
        }
    }
}

pub(super) fn leader_proposal_for_validation(data: &[u8]) -> LeaderProposal {
    match parse_leader_result(data) {
        Ok(outcome) => LeaderProposal::Accepted(outcome),
        Err(vm_error) => LeaderProposal::Rejected(vm_error),
    }
}

pub(super) fn validate_leader_output_after_caps(
    output: &rt::vm::ContractResultBytes,
    leader_proposed: &rt::vm::ContractResultBytes,
) -> Result<(), public_abi::VmError> {
    if output == leader_proposed {
        Ok(())
    } else {
        Err(malformed_leader_result())
    }
}

struct RunNondetGetVMTaskArgs {
    child_topmost_id: runners::Id,
    child_limiter: rt::memlimiter::Limiter,
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
                major: advisory_major.into(),
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
        // Fatality crosses the boundary intact: the caller re-raises it instead
        // of receiving it as a result it could swallow.
        genvm_modules_interfaces::ResultCode::FatalVmError => {
            let data = reply.result.data.materialize()?;
            let calldata::Value::Str(code) = data else {
                anyhow::bail!("nested CallContract fatal VM error is not a string");
            };
            rt::vm::RunOk::FatalVMError(public_abi::VmError(std::borrow::Cow::Owned(code)), None)
        }
        genvm_modules_interfaces::ResultCode::InternalError => {
            anyhow::bail!("nested executor returned an internal error");
        }
    };

    Ok((run_ok, reply.small_hash))
}

impl ContextVFS<'_> {
    pub(super) async fn gl_call_external_call(
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

        let pre_limit = self.context.data.limiter.get_remaining_memory();

        let data = supervisor
            .host
            .lock_for(host::host_fns::Methods::ExternalCall)
            .await
            .external_call(address, &calldata, pre_limit)
            .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

        let Some(data) = data else {
            return Err(generated::types::Error::trap(
                rt::errors::Error::vm(abi::consts::VmError::out_of().memory().val()).into(),
            ));
        };

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
            message_fee_allocation_consumed: Vec::new(),
        };

        let vm_data = Box::new(SingleVMData {
            limiter: args.child_limiter,
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
                    can_use_balance_for_message_fees: false,
                },
                execution: base::Execution {
                    state_mode: public_abi::StorageView::Default,
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
        state: public_abi::StorageView,
        code_slot: SlotID,
        limiter: rt::memlimiter::Limiter,
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
            limiter,
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
                        on: if state == public_abi::StorageView::LatestFinalized {
                            runners::ChainState::Finalized
                        } else {
                            runners::ChainState::Decided
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
                message_fee_allocation_consumed: Vec::new(),
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
        catch_vm_error: bool,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        use genvm_modules_interfaces::{
            NestedPermissions as P, NestedRunEnvelope, NestedRunnerId, NestedStorageType,
        };

        let host_remaining_time_fee_gen_wei = vm_data
            .supervisor
            .host
            .lock_for(host::host_fns::Methods::GetRemainingTimeFeeGenWei)
            .await
            .get_remaining_time_fee_gen_wei()
            .map_err(|e| cross_major_internal("reading nested deterministic fuel", e))?;
        let remaining_det_fuel = vm_data
            .supervisor
            .shared_data
            .remaining_det_fuel(host_remaining_time_fee_gen_wei)
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
        // This line does not gate runner registration, and the bit only
        // widens what a peer line may do with a set it could rebuild anyway.
        permissions |= P::REGISTER_RUNNERS;
        if vm_data.conf.permissions.can_use_balance_for_message_fees {
            permissions |= P::USE_BALANCE_FOR_MESSAGE_FEES;
        }

        let state_mode = match vm_data.conf.execution.state_mode {
            public_abi::StorageView::Default => NestedStorageType::Default,
            public_abi::StorageView::LatestFinalized => NestedStorageType::LatestFinalized,
            public_abi::StorageView::LatestDecided => NestedStorageType::LatestDecided,
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
                transaction_timestamp: message.datetime,
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
        self.publish_sub_vm_result(run_ok, catch_vm_error)
    }

    pub(super) async fn gl_call_contract(
        &mut self,
        address: calldata::Address,
        calldata: abi::entry::MainCallData,
        mut state: public_abi::StorageView,
        catch_vm_error: bool,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        if !self.context.data.conf.permissions.deterministic {
            return Err(generated::types::Errno::Forbidden.into());
        }
        if !self.context.data.conf.permissions.call_others {
            return Err(generated::types::Errno::Forbidden.into());
        }

        let supervisor = self.context.data.supervisor.clone();

        if state == public_abi::StorageView::Default {
            state = self.context.data.conf.execution.state_mode;
        }

        let child_limiter = self.context.limiter.derived();
        let mut child_storage = rt::vm::storage::Storage::new(
            address,
            supervisor.get_storage_limiter(),
            child_limiter.clone(),
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
            .lock_for(host::host_fns::Methods::ResolveCallContractExecutor)
            .await
            .resolve_call_contract_executor(address, state, advisory_major)
            .map_err(|e| cross_major_internal("resolving CallContract executor", e))?;
        let route = call_contract_route(routing_payload, advisory_major);
        let vm_data = self.derive_call_contract_vm_data(
            address,
            &calldata,
            state,
            code_slot,
            child_limiter,
            child_storage,
        );

        match route {
            CallContractRoute::Nested(routing_payload) => {
                if vm_data.remaining_recursion == 0 {
                    return Err(internal_trap(rt::errors::Error::vm(
                        public_abi::VmError::out_of().subvm_recursion(),
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
                self.run_nested_call_contract(routing_payload, vm_data, catch_vm_error)
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

                self.publish_sub_vm_result(res.run_ok, catch_vm_error)
            }
        }
    }

    pub(super) async fn run_nondet(
        &mut self,
        data_leader: bytes::Bytes,
        data_validator: bytes::Bytes,
        runner: Option<String>,
        custom_runners: Option<Vec<String>>,
        catch_vm_error: bool,
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
            Some(r) => rt::supervisor::actions::resolve_runner_id(
                &supervisor,
                &self.context.limiter,
                &parent_runner_id,
                r,
            )
            .await
            .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?,
        };
        let child_custom = rt::supervisor::actions::resolve_child_custom_runners(
            &self.context.loaded,
            custom_runners,
            &child_topmost_id,
        )
        .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

        let memory_error = NondetOutput::vm_error(abi::consts::VmError::out_of().memory().val());
        let fee_error =
            NondetOutput::vm_error(abi::consts::VmError::out_of().receipt().nondet_output());
        preflight_nondet_output_ram(&self.context.limiter, &memory_error, &fee_error)?;
        preflight_nondet_output_fees(
            &self.context.data.supervisor.shared_data.data_fees_limit,
            &memory_error,
            &fee_error,
        )
        .await?;

        let call_no = self
            .context
            .data
            .supervisor
            .nondet_call_no
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        if call_no >= top_limits::NONDET_BLOCKS {
            return Err(generated::types::Error::trap(crate::anyhow_to_wasmtime(
                rt::errors::Error::vm(abi::consts::VmError::out_of().nondet_blocks()).into(),
            )));
        }

        let is_leader = self.context.data.supervisor.shared_data.run_mode == rt::RunMode::Leader;
        let mut child_resources = Some((child_topmost_id, child_custom));
        // The child gets the caller's budget before this block's output charge.
        // The snapshot also keeps queued validator work independent of its parent.
        let mut child_limiter = Some(self.context.limiter.derived());
        let mut validator_proposal = None;

        let output = if is_leader {
            let (child_topmost_id, child_custom) = child_resources
                .take()
                .expect("nondeterministic child resources are available");
            let child_limiter = child_limiter
                .take()
                .expect("nondeterministic child limiter is available");
            let storage_checkpoint = self
                .context
                .data
                .storage
                .fork(child_limiter.clone())
                .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e.into())))?;
            let task_args = RunNondetGetVMTaskArgs {
                child_topmost_id,
                storage_checkpoint,
                child_limiter,
                child_custom,
                call_no,
            };
            let vm_ext_msg = self.context.data.message_data.fork_leader(
                public_abi::EntryKind::ConsensusStage,
                data_leader,
                None,
            );

            let task = self.run_nondet_get_vm_task(vm_ext_msg, task_args);

            let computed_result = task
                .run_now(&self.context.data.supervisor)
                .await
                .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

            let computed_result = leader_outcome_for_publication(computed_result)
                .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e.into())))?;
            NondetOutput::from_outcome(computed_result)
        } else {
            let leaders_res_bytes = self
                .context
                .data
                .supervisor
                .get_leader_nondet_result(call_no);

            // Absent and empty must produce the same outcome (`Supervisor`
            // pads gaps with empty `Bytes`), which is exactly what the empty
            // slice already yields -- so one call covers both.
            // A leader fault is fatal: it says this node's whole run is built on
            // a result no honest leader could have produced, so a caller must
            // not be able to carry on as if the block had merely failed.
            let proposal = leader_proposal_for_validation(&leaders_res_bytes.unwrap_or_default());

            match &proposal {
                // Rejecting is already the disagreement; putting it to the
                // contract's principle would let a `True` vote it away
                LeaderProposal::Rejected(_)
                    if self.context.data.supervisor.shared_data.run_mode
                        == rt::RunMode::Validator =>
                {
                    rt::supervisor::mark_nondet_disagreement(&self.context.data.supervisor, call_no)
                }
                LeaderProposal::Rejected(_) => {}
                LeaderProposal::Accepted(leaders_res)
                    if self.context.data.supervisor.shared_data.run_mode
                        == rt::RunMode::Validator =>
                {
                    validator_proposal = Some(leaders_res.duplicate());
                }
                LeaderProposal::Accepted(_) => {}
            }

            let (result, encoded) = proposal.into_result_and_encoding();
            NondetOutput { result, encoded }
        };

        let leader_proposed_encoding = output.encoded.clone();
        let output = charge_nondet_output(
            &self.context.limiter,
            &self.context.data.supervisor.shared_data.data_fees_limit,
            output,
            &memory_error,
            &fee_error,
        )
        .await?;

        if let Some(leaders_res) = validator_proposal {
            if let Err(error) =
                validate_leader_output_after_caps(&output.encoded, &leader_proposed_encoding)
            {
                rt::supervisor::mark_nondet_disagreement(&self.context.data.supervisor, call_no);
                return Err(generated::types::Error::trap(crate::anyhow_to_wasmtime(
                    rt::errors::Error::fatal_vm(error).into(),
                )));
            }

            let (child_topmost_id, child_custom) = child_resources
                .take()
                .expect("nondeterministic child resources are available");
            let child_limiter = child_limiter
                .take()
                .expect("nondeterministic child limiter is available");
            let storage_checkpoint = self
                .context
                .data
                .storage
                .fork(child_limiter.clone())
                .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e.into())))?;
            let task_args = RunNondetGetVMTaskArgs {
                child_topmost_id,
                storage_checkpoint,
                child_limiter,
                child_custom,
                call_no,
            };
            let vm_ext_msg = self.context.data.message_data.fork_leader(
                public_abi::EntryKind::ConsensusStage,
                data_validator,
                Some(leaders_res),
            );
            let task = self.run_nondet_get_vm_task(vm_ext_msg, task_args);
            rt::supervisor::submit_nondet_vm_task(&self.context.data.supervisor, task).await;
        }

        if is_leader {
            self.context
                .data
                .supervisor
                .push_nondet_result(call_no, output.encoded.clone())
                .await;
        }

        self.publish_sub_vm_result_encoded(
            output.result,
            output.encoded.into_bytes(),
            catch_vm_error,
        )
    }

    pub(super) async fn sandbox(
        &mut self,
        data: bytes::Bytes,
        runner: String,
        allow_write_storage: bool,
        allow_send_messages: bool,
        custom_runners: Option<Vec<String>>,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        let supervisor = self.context.data.supervisor.clone();

        let parent_runner_id = self.context.data.conf.execution.topmost_runner_id.clone();
        let topmost_runner_id = rt::supervisor::actions::resolve_runner_id(
            &supervisor,
            &self.context.limiter,
            &parent_runner_id,
            &runner,
        )
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

        let child_limiter = self.context.limiter.derived();
        let storage_checkpoint = self
            .context
            .data
            .storage
            .fork(child_limiter.clone())
            .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e.into())))?;

        let mut fake_my_data = VMDataAccumulator {
            data_fees_limit: self.context.data.accumulator.data_fees_limit.clone(),
            messages_value_decremented: primitive_types::U256::max_value(),
            emissions: Vec::new(),
            message_fee_allocation: Vec::new(),
            message_fee_allocation_consumed: Vec::new(),
        };

        std::mem::swap(&mut self.context.data.accumulator, &mut fake_my_data);

        let stolen_data = fake_my_data;

        let vm_data = Box::new(SingleVMData {
            limiter: child_limiter,
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
        self.context
            .data
            .storage
            .fold(my_res.vm_data.storage)
            .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e.into())))?;

        let data = my_res
            .run_ok
            .into_contract_observable_bytes()
            .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e.into())))?;
        self.place_content(vfs::FileContents::from(bytes::Bytes::from(data)))
    }
}
