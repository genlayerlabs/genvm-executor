use std::collections::BTreeMap;
use std::sync::Arc;

use genvm_common::sync::DArc;
use genvm_common::*;

use genvm_modules_interfaces::GenericValue;
use sha3::digest::Update;
use wiggle::GuestError;

use crate::host::{self, SlotID};
use crate::{anyhow_to_wasmtime, calldata, domain, public_abi, rt, runners, wasi};

use genlayer_calldata::codec::Encode;
pub use genlayer_sdk::abi::entry::ExtendedMessage;
use genlayer_sdk::abi::{self, gl_call};

use super::{base, vfs};

fn default_entry_stage_data() -> calldata::Value {
    calldata::Value::Null
}

fn oom_trap(error: abi::consts::VmError) -> generated::types::Error {
    generated::types::Error::trap(crate::anyhow_to_wasmtime(
        rt::errors::Error::vm(error).into(),
    ))
}

fn cross_major_internal(operation: &str, error: impl std::fmt::Display) -> generated::types::Error {
    generated::types::Error::trap(crate::anyhow_to_wasmtime(
        rt::errors::Error::internal(format!("{operation}: {error}")).into(),
    ))
}

fn nested_run_ok(
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
            let data = reply.result.data.materialize()?;
            match data {
                calldata::Value::Str(message) => rt::vm::RunOk::UserError(message),
                payload => {
                    // User errors on this line are strings, so a callee raising
                    // structured data produced something this ABI cannot carry.
                    // That is the callee's ordinary failure, not a fault, so it
                    // must stay a contract-visible error rather than become an
                    // internal one. The payload is dropped and survives only
                    // here, which is the sole record of what actually failed.
                    log_error!(payload:? = payload; "nested CallContract user error is not a string");
                    rt::vm::RunOk::VMError(
                        public_abi::VmError::invalid_contract().major_mismatch(),
                        None,
                    )
                }
            }
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

/// Named arguments for [`consume_message_fee_internal`].
struct ConsumeInternalArgs {
    is_deploy: bool,
    calldata_length: u64,
    code_length: u64,
    subtree_length: u64,
}

async fn consume_message_fee_internal(
    shared_data: &rt::SharedData,
    node: &mut domain::fees::MessageAllocationNode,
    fee_params: Arc<domain::fees::InternalMessageParams>,
    on: gl_call::On,
    args: ConsumeInternalArgs,
) -> Result<rt::fees::MessageFeeConsumption, generated::types::Error> {
    let fee_cost = shared_data
        .data_fees_limit
        .calculate_message_fee_internal(on, &fee_params)
        .map_err(|x| generated::types::Error::trap(anyhow_to_wasmtime(x)))?;

    let fee_total = fee_cost.sum();
    if fee_total > node.budget {
        log_warn!(
            node:cd = *node,
            fee_cost:cd = fee_total,
            budget: cd = node.budget;
            "message fee cost exceeds node budget"
        );
        return Err(oom_trap(abi::consts::VmError::oom().fees().internal()));
    }

    let receipt_cost = shared_data
        .data_fees_limit
        .calculate_message_receipt(rt::fees::MessageReceiptParams {
            is_internal: true,
            is_deploy: args.is_deploy,
            calldata_length: args.calldata_length,
            code_length: args.code_length,
            subtree_length: args.subtree_length,
        })
        .map_err(|x| {
            generated::types::Error::trap(anyhow_to_wasmtime(
                x.context("calculate_message_receipt"),
            ))
        })?;

    if !shared_data
        .data_fees_limit
        .consume_message_fee(&fee_cost, &receipt_cost)
        .await
    {
        log_warn!(
            node:cd = *node,
            fee_cost:cd = fee_total,
            buckets:? = shared_data.data_fees_limit;
            "not enough remaining fee limit to consume message fee"
        );
        return Err(oom_trap(abi::consts::VmError::oom().fees().internal()));
    }

    node.budget -= fee_total;

    Ok(rt::fees::MessageFeeConsumption {
        message_fee: fee_cost,
        receipt_fee: receipt_cost,
    })
}

/// Named arguments for [`consume_message_fee_external`].
struct ConsumeExternalArgs {
    is_deploy: bool,
    calldata_length: u64,
}

async fn consume_message_fee_external(
    shared_data: &rt::SharedData,
    node: &mut domain::fees::MessageAllocationNode,
    params: domain::fees::ExternalMessageParams,
    // External messages are always emitted on finalization; carried for signature
    // symmetry with the internal path.
    _on: gl_call::On,
    args: ConsumeExternalArgs,
) -> Result<rt::fees::MessageFeeConsumption, generated::types::Error> {
    let fee_cost = shared_data
        .data_fees_limit
        .calculate_message_fee_external(&params)
        .map_err(|x| generated::types::Error::trap(anyhow_to_wasmtime(x)))?;

    let fee_total = fee_cost.sum();
    if fee_total > node.budget {
        return Err(oom_trap(abi::consts::VmError::oom().fees().external()));
    }

    let receipt_cost = shared_data
        .data_fees_limit
        .calculate_message_receipt(rt::fees::MessageReceiptParams {
            is_internal: false,
            is_deploy: args.is_deploy,
            calldata_length: args.calldata_length,
            code_length: 0,
            subtree_length: 0,
        })
        .map_err(|x| generated::types::Error::trap(anyhow_to_wasmtime(x)))?;

    if !shared_data
        .data_fees_limit
        .consume_message_fee(&fee_cost, &receipt_cost)
        .await
    {
        return Err(oom_trap(abi::consts::VmError::oom().fees().external()));
    }

    node.budget -= fee_total;

    Ok(rt::fees::MessageFeeConsumption {
        message_fee: fee_cost,
        receipt_fee: receipt_cost,
    })
}

async fn consume_nondet_output(
    shared_data: &rt::SharedData,
    output_length: u64,
) -> Result<(), generated::types::Error> {
    if !shared_data
        .data_fees_limit
        .consume_nondet_output(output_length)
        .await
        .map_err(|x| generated::types::Error::trap(anyhow_to_wasmtime(x)))?
    {
        return Err(oom_trap(
            abi::consts::VmError::oom().receipt().nondet_output(),
        ));
    }
    Ok(())
}

/// Extension methods for ExtendedMessage specific to the executor
pub trait ExtendedMessageExt {
    fn fork_leader(
        &self,
        entry_kind: public_abi::EntryKind,
        entry_data: bytes::Bytes,
        entry_leader_data: Option<rt::vm::RunOk>,
    ) -> ExtendedMessage;

    fn fork(&self, entry_kind: public_abi::EntryKind, entry_data: bytes::Bytes) -> ExtendedMessage;
}

impl ExtendedMessageExt for ExtendedMessage {
    fn fork_leader(
        &self,
        entry_kind: public_abi::EntryKind,
        entry_data: bytes::Bytes,
        entry_leader_data: Option<rt::vm::RunOk>,
    ) -> ExtendedMessage {
        use genlayer_sdk::abi::entry::MessageData;

        let entry_leader_data = match entry_leader_data {
            None => default_entry_stage_data(),
            Some(entry_leader_data) => calldata::Value::Map(BTreeMap::from([(
                "leaders_result".into(),
                calldata::Value::Bytes(entry_leader_data.as_bytes()),
            )])),
        };

        ExtendedMessage {
            message: MessageData {
                contract_address: self.message.contract_address,
                sender_address: self.message.sender_address,
                origin_address: self.message.origin_address,
                stack: self.message.stack.clone(),
                chain_id: self.message.chain_id.clone(),
                value: self.message.value.clone(),
                is_init: false,
                datetime: self.message.datetime,
            },
            entry_kind,
            entry_data,
            entry_stage_data: entry_leader_data,
        }
    }

    fn fork(&self, entry_kind: public_abi::EntryKind, entry_data: bytes::Bytes) -> ExtendedMessage {
        self.fork_leader(entry_kind, entry_data, None)
    }
}

#[derive(Clone)]
pub struct ReadToken {
    pub mode: public_abi::StorageType,
    pub account: calldata::Address,
}

pub struct StorageHostLock<'a>(tokio::sync::MutexGuard<'a, host::Host>, ReadToken);

impl rt::vm::storage::HostStorage for StorageHostLock<'_> {
    fn storage_read(&mut self, slot_id: SlotID, index: u32, buf: &mut [u8]) -> anyhow::Result<()> {
        self.0
            .storage_read(self.1.mode, self.1.account, slot_id, index, buf)
    }
}

#[derive(Clone)]
pub struct StorageHostHolder(pub Arc<host::MultiHost>, pub ReadToken);

impl rt::vm::storage::HostStorageLocking for StorageHostHolder {
    type ReturnType<'a> = StorageHostLock<'a>;

    async fn lock(&self) -> Self::ReturnType<'_> {
        StorageHostLock(
            self.0.lock_for(host::host_fns::Methods::StorageRead).await,
            self.1.clone(),
        )
    }
}

pub struct VMDataAccumulator {
    pub data_fees_limit: DArc<rt::fees::DataLimit>,
    pub messages_value_decremented: primitive_types::U256,
    pub emissions: Vec<domain::ExecutionEmission>,
    pub message_fee_allocation: Vec<domain::fees::MessageAllocationNode>,
    /// Custom runner hashes registered in (and inherited into) this execution
    /// scope. Only these may be resolved via `custom:<hash>`. A nondet sub-VM
    /// starts empty, so it cannot see runners the deterministic scope registered.
    pub custom_runners: rpds::HashTrieSet<Bytes32Hash, archery::ArcTK>,
}

impl VMDataAccumulator {
    /// Asserts the accumulator carries no surfaced effects (events,
    /// message-fee allocations). Call it after running a sub-VM whose
    /// accumulator is discarded (e.g. a `CallContract` child) to guarantee no
    /// effect was charged but silently dropped. Returns a fatal error otherwise.
    pub fn check_empty(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.emissions.is_empty() && self.message_fee_allocation.is_empty(),
            "internal error: discarded sub-VM accumulator is not empty ({} emission(s), {} message-fee allocation(s))",
            self.emissions.len(),
            self.message_fee_allocation.len(),
        );
        Ok(())
    }
}

pub struct SingleVMData {
    pub conf: base::Config,
    /// Recursion budget left to this VM and everything it spawns, inherited
    /// from the chain root rather than counted up from zero, so a chain that
    /// crosses executor lines keeps the bound its root minted.
    pub remaining_recursion: u32,
    /// Transaction signer. This line's contract-facing message has no such
    /// field, so it is carried beside the message rather than inside it, to be
    /// forwarded to executors that do expose it.
    pub signer_address: calldata::Address,
    pub message_data: ExtendedMessage,
    pub supervisor: Arc<rt::supervisor::Supervisor>,
    pub storage: rt::vm::storage::Storage<StorageHostHolder>,
    pub accumulator: VMDataAccumulator,
    pub det_subvm_hashes: sha3::Sha3_256,
}

impl SingleVMData {
    /// How deep this VM sits, for logs only. Exact while the chain stays on
    /// one line; an approximation once it crosses into a line whose own limit
    /// differs.
    pub fn depth(&self) -> u32 {
        public_abi::top_limits::VM_RECURSION.saturating_sub(self.remaining_recursion)
    }
}

pub struct Context {
    pub data: SingleVMData,

    pub start_time: std::time::Instant,
    pub prev_time: std::time::Instant,
}

pub struct ContextVFS<'a> {
    pub(super) vfs: &'a mut vfs::VFS,
    pub(super) preview1: &'a mut super::preview1::Context,
    pub(super) context: &'a mut Context,
}

#[allow(clippy::too_many_arguments)]
pub(crate) mod generated {
    wiggle::from_witx!({
        witx: ["$CARGO_MANIFEST_DIR/src/wasi/witx/genlayer_sdk.witx"],
        errors: { errno => trappable Error },
        wasmtime: false,
        tracing: false,

        async: {
            genlayer_sdk::{
                gl_call,
                storage_read, storage_write,
                get_balance, get_self_balance,
            }
        },
    });

    wiggle::wasmtime_integration!({
        witx: ["$CARGO_MANIFEST_DIR/src/wasi/witx/genlayer_sdk.witx"],
        errors: { errno => trappable Error },
        target: self,
        tracing: false,

        async: {
            genlayer_sdk::{
                gl_call,
                storage_read, storage_write,
                get_balance, get_self_balance,
            }
        },
    });
}

impl From<wasi::vfs::Fd> for generated::types::Fd {
    fn from(fd: wasi::vfs::Fd) -> Self {
        fd.as_u32().into()
    }
}

impl From<generated::types::Fd> for wasi::vfs::Fd {
    fn from(fd: generated::types::Fd) -> Self {
        wasi::vfs::Fd::new(fd.into())
    }
}

fn read_addr_from_mem(
    mem: &mut wiggle::GuestMemory<'_>,
    addr: wiggle::GuestPtr<u8>,
) -> Result<calldata::Address, generated::types::Error> {
    let cow = mem.as_cow(
        addr.as_array(
            calldata::ADDRESS_SIZE
                .try_into()
                .expect("ADDRESS_SIZE exceeds target type"),
        ),
    )?;
    let mut ret = calldata::Address::zero();
    ret.ref_mut().copy_from_slice(&cow);
    Ok(ret)
}

impl SlotID {
    fn read_from_mem(
        mem: &mut wiggle::GuestMemory<'_>,
        addr: wiggle::GuestPtr<u8>,
    ) -> Result<Self, generated::types::Error> {
        let cow = mem.as_cow(
            addr.as_array(
                SlotID::len()
                    .try_into()
                    .expect("SlotID::len exceeds target type"),
            ),
        )?;
        let mut ret = SlotID::zero();
        for (x, y) in ret.0.iter_mut().zip(cow.iter()) {
            *x = *y;
        }
        Ok(ret)
    }
}

fn read_owned_vec(
    mem: &mut wiggle::GuestMemory<'_>,
    ptr: wiggle::GuestPtr<[u8]>,
) -> Result<Vec<u8>, generated::types::Error> {
    Ok(mem.as_cow(ptr)?.into_owned())
}

impl Context {
    pub fn new(data: Box<SingleVMData>) -> Self {
        let now = std::time::Instant::now();

        Self {
            data: *data,
            start_time: now,
            prev_time: now,
        }
    }
}

impl wiggle::GuestErrorType for generated::types::Errno {
    fn success() -> Self {
        Self::Success
    }
}

pub trait AddToLinkerFn<T> {
    fn call<'a>(&self, arg: &'a mut T) -> ContextVFS<'a>;
}

pub(super) fn add_to_linker_sync<T: Send + 'static, F>(
    linker: &mut wasmtime::Linker<T>,
    f: F,
) -> anyhow::Result<()>
where
    F: AddToLinkerFn<T> + Copy + Send + Sync + 'static,
{
    #[derive(Clone, Copy)]
    struct Fwd<F>(F);

    impl<T, F> generated::AddGenlayerSdkToLinkerFn<T> for Fwd<F>
    where
        F: AddToLinkerFn<T> + Copy + Send + Sync + 'static,
    {
        fn call(&self, arg: &mut T) -> impl generated::genlayer_sdk::GenlayerSdk {
            self.0.call(arg)
        }
    }
    generated::add_genlayer_sdk_to_linker(linker, Fwd(f))?;
    Ok(())
}

#[derive(Debug)]
pub struct ContractReturn(pub calldata::unparsed::Maybe<calldata::Value>);

impl std::error::Error for ContractReturn {}

impl std::fmt::Display for ContractReturn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Returned {:?}", self.0)
    }
}

impl From<GuestError> for generated::types::Error {
    fn from(err: GuestError) -> Self {
        use wiggle::GuestError::*;
        match err {
            InvalidFlagValue { .. } => generated::types::Errno::Inval.into(),
            InvalidEnumValue { .. } => generated::types::Errno::Inval.into(),
            // As per
            // https://github.com/WebAssembly/wasi/blob/main/legacy/tools/witx-docs.md#pointers
            //
            // > If a misaligned pointer is passed to a function, the function
            // > shall trap.
            // >
            // > If an out-of-bounds pointer is passed to a function and the
            // > function needs to dereference it, the function shall trap.
            //
            // so this turns OOB and misalignment errors into traps.
            PtrOverflow | PtrOutOfBounds { .. } | PtrNotAligned { .. } => {
                generated::types::Error::trap(crate::anyhow_to_wasmtime(err.into()))
            }
            InvalidUtf8 { .. } => generated::types::Errno::Ilseq.into(),
            TryFromIntError { .. } => generated::types::Errno::Overflow.into(),
            SliceLengthsDiffer => generated::types::Errno::Fault.into(),
            InFunc { err, .. } => generated::types::Error::from(*err),
            MemoryNotExported => generated::types::Errno::Fault.into(),
        }
    }
}

impl From<std::num::TryFromIntError> for generated::types::Error {
    fn from(_err: std::num::TryFromIntError) -> Self {
        generated::types::Errno::Overflow.into()
    }
}

impl From<serde_json::Error> for generated::types::Error {
    fn from(err: serde_json::Error) -> Self {
        log_info!(error:err = err; "deserialization failed, returning inval");

        generated::types::Errno::Inval.into()
    }
}

impl ContextVFS<'_> {
    fn place_content(
        &mut self,
        content: vfs::FileContents,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        self.vfs
            .place_content(content)
            .map(generated::types::Fd::from)
            .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))
    }

    fn set_vm_run_result(
        &mut self,
        data: rt::vm::RunOk,
    ) -> Result<(generated::types::Fd, usize), generated::types::Error> {
        let data = match data {
            rt::vm::RunOk::VMError(e, cause) => {
                return Err(generated::types::Error::trap(crate::anyhow_to_wasmtime(
                    rt::errors::Error::vm_cause(e, cause).into(),
                )))
            }
            data => data,
        };
        let data: Vec<u8> = data.as_bytes();
        let len = data.len();
        self.place_content(vfs::FileContents::from(bytes::Bytes::from(data)))
            .map(|fd| (fd, len))
    }
}

async fn taskify<T>(
    fut: impl std::future::Future<Output = anyhow::Result<std::result::Result<T, GenericValue>>>
        + Send
        + 'static,
) -> anyhow::Result<Box<[u8]>>
where
    T: calldata::codec::Encode<Vec<u8>, Error = std::convert::Infallible> + Send,
{
    match fut.await? {
        Ok(r) => {
            let r = calldata::to_value(&r);
            let data = calldata::Value::Map(BTreeMap::from([("ok".to_owned(), r)]));

            Ok(Box::from(calldata::encode(&data)))
        }
        Err(e) => {
            let e = calldata::to_value(&e);
            let data = calldata::Value::Map(BTreeMap::from([("error".to_owned(), e)]));

            Ok(Box::from(calldata::encode(&data)))
        }
    }
}

const NO_FILE: u32 = u32::MAX;

#[inline]
fn file_fd_none() -> generated::types::Fd {
    generated::types::Fd::from(NO_FILE)
}

/// Returns `true` iff `a + b <= c`, computed without wrapping.
///
/// `a + b` is done with checked arithmetic: on overflow the sum cannot fit in a
/// `U256` and is therefore strictly greater than any real `c`, so the result is
/// `false`. This avoids the wraparound that would let an oversized `a` (e.g.
/// `2^256 - 1`) appear to satisfy a balance check.
#[inline]
fn checked_sum_le(
    a: primitive_types::U256,
    b: primitive_types::U256,
    c: primitive_types::U256,
) -> bool {
    if b > c {
        return false;
    }
    let c_minus_b = c - b;
    a <= c_minus_b
}

#[allow(unused_variables)]
impl generated::genlayer_sdk::GenlayerSdk for ContextVFS<'_> {
    async fn gl_call(
        &mut self,
        mem: &mut wiggle::GuestMemory<'_>,
        request: wiggle::GuestPtr<u8>,
        request_len: u32,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        let request = request.as_array(request_len);
        let request = read_owned_vec(mem, request)?;

        let request = match calldata::decode(&request) {
            Err(e) => {
                log_info!(error:err = &e; "calldata parse failed");

                return Err(generated::types::Errno::Inval.into());
            }
            Ok(v) => v,
        };

        log_trace!(request:cd = request; "gl_call");

        let request: gl_call::Message = match calldata::from_value(request) {
            Ok(v) => v,
            Err(e) => {
                log_info!(error:err = e; "calldata deserialization failed");

                return Err(generated::types::Errno::Inval.into());
            }
        };

        match request {
            gl_call::Message::EthSend {
                address,
                calldata,
                value,
            } => {
                if !self.context.data.conf.is_deterministic {
                    return Err(generated::types::Errno::Forbidden.into());
                }
                if !self.context.data.conf.can_send_messages {
                    return Err(generated::types::Errno::Forbidden.into());
                }

                if !value.is_zero() {
                    let my_balance = self
                        .context
                        .get_balance_impl(self.context.data.message_data.message.contract_address)
                        .await?;

                    if !checked_sum_le(
                        value,
                        self.context.data.accumulator.messages_value_decremented,
                        my_balance,
                    ) {
                        return Err(generated::types::Errno::Inbalance.into());
                    }
                }

                let mut call_key = abi::CallKey([0u8; 32]);
                if calldata.len() < 4 {
                    log_warn!(len = calldata.len(); "calldata too short for method selector, using unnamed call key");
                } else {
                    call_key.0[..4].copy_from_slice(&calldata[..4]);
                }

                let Some((matched_node, matched_params)) = self
                    .context
                    .data
                    .accumulator
                    .message_fee_allocation
                    .iter_mut()
                    .find_map(|node| {
                        node.matches_external(address, call_key)
                            .map(|params| (node, params))
                    })
                else {
                    log_warn!(
                        recipient = address,
                        call_key:? = call_key;
                        "no matching node for message fee allocation"
                    );

                    return Err(oom_trap(abi::consts::VmError::oom().fees().external()));
                };

                let calldata_length = calldata.len() as u64;

                let fees = consume_message_fee_external(
                    &self.context.data.supervisor.shared_data,
                    matched_node,
                    matched_params,
                    gl_call::On::Finalized,
                    ConsumeExternalArgs {
                        is_deploy: false,
                        calldata_length,
                    },
                )
                .await?;

                self.context
                    .data
                    .accumulator
                    .emissions
                    .push(domain::ExecutionEmission::EthSend {
                        address,
                        calldata,
                        value,
                        message_fee: fees.message_fee.sum(),
                        receipt_fee: fees.receipt_fee.sum(),
                        fee_params: matched_params,
                    });

                self.context.data.accumulator.messages_value_decremented = self
                    .context
                    .data
                    .accumulator
                    .messages_value_decremented
                    .saturating_add(value);
                Ok(file_fd_none())
            }
            gl_call::Message::EthCall { address, calldata } => {
                if !self.context.data.conf.is_deterministic {
                    return Err(generated::types::Errno::Forbidden.into());
                }
                if !self.context.data.conf.can_call_others {
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
            gl_call::Message::CallContract {
                address,
                calldata,
                mut state,
            } => {
                if !self.context.data.conf.is_deterministic {
                    return Err(generated::types::Errno::Forbidden.into());
                }
                if !self.context.data.conf.can_call_others {
                    return Err(generated::types::Errno::Forbidden.into());
                }

                if state == public_abi::StorageType::Default {
                    state = match self.context.data.conf.state_mode {
                        public_abi::StorageType::Default => public_abi::StorageType::LatestNonFinal,
                        inherited => inherited,
                    };
                }

                let supervisor = self.context.data.supervisor.clone();

                let my_conf = self.context.data.conf.clone();

                let calldata_encoded = calldata::encode_obj(&calldata);

                let mut my_data = self
                    .context
                    .data
                    .message_data
                    .fork(public_abi::EntryKind::Main, calldata_encoded.into());
                my_data.message.stack.push(my_data.message.contract_address);

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

                // Roots on this line carry no major field, so the executor
                // has nothing to read: it reports the never-written default and
                // leaves the decision entirely to the host.
                const ADVISORY_MAJOR: u8 = 0;

                let routing_payload = supervisor
                    .host
                    .lock_for(host::host_fns::Methods::ResolveCallcontractExecutor)
                    .await
                    .resolve_callcontract_executor(address, state, ADVISORY_MAJOR)
                    .map_err(|e| cross_major_internal("resolving CallContract executor", e))?;

                let code_slot = child_storage
                    .check_major_and_resolve_code_slot()
                    .await
                    .map_err(|e| {
                        generated::types::Error::trap(crate::anyhow_to_wasmtime(e.into()))
                    })?;

                let vm_data = Box::new(SingleVMData {
                    remaining_recursion: self.context.data.remaining_recursion.saturating_sub(1),
                    signer_address: self.context.data.signer_address,
                    // Permission model: docs/website/src/spec/03-vm/02-meta-properties.rst
                    conf: base::Config {
                        needs_error_fingerprint: true,
                        is_deterministic: true,
                        can_read_storage: my_conf.can_read_storage,
                        can_write_storage: false,
                        can_spawn_nondet: my_conf.can_spawn_nondet,
                        can_call_others: my_conf.can_call_others,
                        can_send_messages: false,
                        can_register_runners: my_conf.can_register_runners,
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
                    message_data: ExtendedMessage {
                        message: genlayer_sdk::abi::entry::MessageData {
                            contract_address: address,
                            sender_address: my_data.message.sender_address,
                            origin_address: my_data.message.origin_address,
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
                    supervisor: supervisor.clone(),
                    accumulator: VMDataAccumulator {
                        data_fees_limit: self.context.data.accumulator.data_fees_limit.clone(),
                        messages_value_decremented: self
                            .context
                            .data
                            .accumulator
                            .messages_value_decremented,
                        emissions: Vec::new(),
                        message_fee_allocation: Vec::new(),
                        // CallContract is a deterministic sub-call: inherit the
                        // runners registered so far.
                        custom_runners: self.context.data.accumulator.custom_runners.clone(),
                    },
                    det_subvm_hashes: Default::default(),
                });

                if let Some(routing_payload) = routing_payload {
                    if vm_data.remaining_recursion == 0 {
                        return Err(oom_trap(public_abi::VmError::oom().val()));
                    }
                    // Custom runners are process-local: the envelope carries no
                    // archives, so a callee in another executor could not load
                    // them and would silently run with an empty set. Refuse the
                    // call instead, so the caller sees a canonical error rather
                    // than a child that resolves `custom:` ids differently per
                    // route.
                    if !vm_data.accumulator.custom_runners.is_empty() {
                        return Err(generated::types::Errno::Inval.into());
                    }
                    use genvm_modules_interfaces::{
                        NestedPermissions as P, NestedRunEnvelope, NestedRunnerId,
                        NestedStorageType,
                    };

                    let mut nested_calldata = calldata::decode(&vm_data.message_data.entry_data)
                        .map_err(|e| {
                            cross_major_internal("decoding nested CallContract calldata", e)
                        })?;
                    super::method_compat::method_legacy_to_new(&mut nested_calldata);
                    let nested_calldata = bytes::Bytes::from(calldata::encode(&nested_calldata));

                    let host_remaining_fuel = supervisor
                        .host
                        .lock_for(host::host_fns::Methods::RemainingFuelAsGen)
                        .await
                        .remaining_fuel_as_gen()
                        .map_err(|e| {
                            cross_major_internal("reading nested deterministic fuel", e)
                        })?;
                    let remaining_det_fuel = supervisor
                        .shared_data
                        .remaining_det_fuel(host_remaining_fuel)
                        .await;

                    let mut permissions = P::default();
                    if vm_data.conf.is_deterministic {
                        permissions |= P::DETERMINISTIC;
                    }
                    if vm_data.conf.can_read_storage {
                        permissions |= P::READ_STORAGE;
                    }
                    if vm_data.conf.can_write_storage {
                        permissions |= P::WRITE_STORAGE;
                    }
                    if vm_data.conf.can_send_messages {
                        permissions |= P::SEND_MESSAGES;
                    }
                    if vm_data.conf.can_call_others {
                        permissions |= P::CALL_OTHERS;
                    }
                    // This line lets a `CallContract` child inherit the parent's
                    // nondet permission; the boundary does not, so a callee on
                    // another line is never handed authority its own derivation
                    // would have cleared.
                    if vm_data.conf.can_register_runners {
                        permissions |= P::REGISTER_RUNNERS;
                    }

                    let state_mode = match vm_data.conf.state_mode {
                        public_abi::StorageType::Default => NestedStorageType::Default,
                        public_abi::StorageType::LatestFinal => NestedStorageType::LatestFinal,
                        public_abi::StorageType::LatestNonFinal => {
                            NestedStorageType::LatestNonFinal
                        }
                    };
                    let message = &vm_data.message_data.message;
                    let envelope = NestedRunEnvelope {
                        routing_payload,
                        calldata: nested_calldata,
                        message: genvm_modules_interfaces::MessageData {
                            contract_address: message.contract_address,
                            sender_address: message.sender_address,
                            origin_address: message.origin_address,
                            signer_address: vm_data.signer_address,
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
                        memory_limit: supervisor.limiter.get(true).get_remaining_memory(),
                    };

                    let reply = supervisor
                        .host
                        .lock_for(host::host_fns::Methods::RunNested)
                        .await
                        .run_nested(&envelope)
                        .map_err(|e| cross_major_internal("running nested executor", e))?;
                    let (run_ok, small_hash) = nested_run_ok(reply)
                        .map_err(|e| cross_major_internal("reading nested result", e))?;

                    self.context.data.det_subvm_hashes.update(&small_hash);
                    return self.set_vm_run_result(run_ok).map(|x| x.0);
                }

                let res = rt::spawn_apply_run(&supervisor, vm_data)
                    .await
                    .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

                // The child is read-only (static), so its accumulator must be
                // empty — otherwise an effect was charged but discarded here.
                res.vm_data
                    .accumulator
                    .check_empty()
                    .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

                let hash = res.small_hash();
                self.context.data.det_subvm_hashes.update(&hash);

                self.set_vm_run_result(res.run_ok).map(|x| x.0)
            }
            gl_call::Message::EmitEvent { topics, blob } => {
                if !self.context.data.conf.is_deterministic {
                    log_warn!("EmitEvent requires deterministic mode");

                    return Err(generated::types::Errno::Forbidden.into());
                }
                // Events are state-mutating log emissions, so they require the
                // same write capability as storage. A read-only sub-VM (e.g. a
                // `CallContract` child) must not emit events; otherwise the
                // emission is charged but later discarded with the child.
                if !self.context.data.conf.can_write_storage {
                    log_warn!("EmitEvent requires write_storage permission");

                    return Err(generated::types::Errno::Forbidden.into());
                }

                if topics.len() > public_abi::EVENT_MAX_TOPICS as usize {
                    log_warn!(cnt = topics.len(), max = public_abi::EVENT_MAX_TOPICS; "too many topics");
                    return Err(generated::types::Errno::Inval.into());
                }

                let mut real_topics: Vec<bytes::Bytes> =
                    Vec::with_capacity(public_abi::EVENT_MAX_TOPICS as usize + 1);

                for (i, t) in topics.iter().enumerate() {
                    if t.len() != 32 {
                        log_warn!(len = t.len(); "invalid topic length");

                        return Err(generated::types::Errno::Inval.into());
                    }

                    real_topics.push(t.clone());
                }

                struct CountingWriter(usize);
                impl calldata::Writer for CountingWriter {
                    type Error = std::convert::Infallible;

                    fn write_all(&mut self, data: &[u8]) -> Result<(), Self::Error> {
                        self.0 += data.len();
                        Ok(())
                    }
                }

                let mut enc = calldata::Encoder::new(CountingWriter(0));
                blob.encode(&mut enc).unwrap_or_else(|e| match e {});
                let blob_size = enc.into_inner().0 as u64;

                let supervisor = self.context.data.supervisor.clone();
                let topics_count = topics.len() as u64;

                let storage_fee = supervisor
                    .shared_data
                    .data_fees_limit
                    .consume_event(blob_size, topics_count)
                    .await
                    .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?
                    .ok_or_else(|| oom_trap(abi::consts::VmError::oom().storage()))?;

                self.context.data.accumulator.emissions.push(
                    domain::ExecutionEmission::EmitEvent {
                        topics: real_topics,
                        blob,
                        storage_fee,
                    },
                );

                Ok(file_fd_none())
            }
            gl_call::Message::PostMessage {
                address,
                calldata,
                value,
                on,
            } => {
                log_debug!(
                    recipient = address,
                    on:? = on;
                    "PostMessage dispatched"
                );
                if !self.context.data.conf.is_deterministic {
                    log_debug!("PostMessage rejected: non-deterministic context (Forbidden)");
                    return Err(generated::types::Errno::Forbidden.into());
                }
                if !self.context.data.conf.can_send_messages {
                    log_debug!("PostMessage rejected: can_send_messages=false (Forbidden)");
                    return Err(generated::types::Errno::Forbidden.into());
                }

                let sd = self.context.data.supervisor.shared_data.clone();

                // Peek the (possibly deferred) calldata to derive the fee call key.
                let calldata_materialized = calldata.clone().materialize().ok();
                // This calldata is built by the runner, which writes the method
                // name under the legacy "method" key (gl/genvm_contracts.py:46).
                let method_name = calldata_materialized
                    .as_ref()
                    .and_then(|x| x.as_map())
                    .and_then(|x| x.get(super::method_compat::LEGACY_METHOD_KEY))
                    .and_then(|x| x.as_str());
                let call_key = if let Some(method_name) = method_name {
                    abi::CallKey::for_method(method_name)
                } else {
                    abi::CallKey::UNNAMED
                };

                if !value.is_zero() {
                    let my_balance = self
                        .context
                        .get_balance_impl(self.context.data.message_data.message.contract_address)
                        .await?;

                    if !checked_sum_le(
                        value,
                        self.context.data.accumulator.messages_value_decremented,
                        my_balance,
                    ) {
                        return Err(generated::types::Errno::Inbalance.into());
                    }
                }

                let Some((matched_node, matched_params)) = self
                    .context
                    .data
                    .accumulator
                    .message_fee_allocation
                    .iter_mut()
                    .find_map(|node| {
                        node.matches_internal(on, address, call_key)
                            .map(|params| (node, params))
                    })
                else {
                    log_warn!(
                        recipient = address,
                        call_key:? = call_key,
                        on:? = on;
                        "no matching node for message fee allocation"
                    );

                    return Err(oom_trap(abi::consts::VmError::oom().fees().internal()));
                };

                log_debug!(
                    recipient = address,
                    call_key:? = call_key,
                    on:? = on;
                    "PostMessage matched fee allocation node"
                );

                let mut enc = calldata::Encoder::new(calldata::CounterWriter(0));
                calldata::codec::Encode::encode(&calldata, &mut enc).unwrap_or_else(|e| match e {});
                let calldata_length = enc.into_inner().0;

                let fee_params = (*matched_params).clone();
                let subtree = bytes::Bytes::from(domain::fees::MessageAllocationNode::abi_encode(
                    &matched_node.children,
                ));

                let fees = consume_message_fee_internal(
                    &self.context.data.supervisor.shared_data,
                    matched_node,
                    matched_params,
                    on,
                    ConsumeInternalArgs {
                        is_deploy: false,
                        calldata_length,
                        code_length: 0,
                        subtree_length: subtree.len() as u64,
                    },
                )
                .await?;

                self.context.data.accumulator.emissions.push(
                    domain::ExecutionEmission::PostMessage {
                        call_key,
                        address,
                        calldata,
                        value,
                        on,
                        message_fee: fees.message_fee.sum(),
                        receipt_fee: fees.receipt_fee.sum(),
                        fee_params,
                        subtree,
                    },
                );

                log_debug!(
                    depth = self.context.data.depth(),
                    emissions_total = self.context.data.accumulator.emissions.len();
                    "PostMessage emission pushed to accumulator"
                );

                self.context.data.accumulator.messages_value_decremented = self
                    .context
                    .data
                    .accumulator
                    .messages_value_decremented
                    .saturating_add(value);

                Ok(file_fd_none())
            }
            gl_call::Message::DeployContract {
                calldata,
                code,
                value,
                on,
                salt_nonce,
            } => {
                if !self.context.data.conf.is_deterministic {
                    return Err(generated::types::Errno::Forbidden.into());
                }
                if !self.context.data.conf.can_send_messages {
                    return Err(generated::types::Errno::Forbidden.into());
                }

                let sd = self.context.data.supervisor.shared_data.clone();

                if !value.is_zero() {
                    let my_balance = self
                        .context
                        .get_balance_impl(self.context.data.message_data.message.contract_address)
                        .await?;

                    if !checked_sum_le(
                        value,
                        self.context.data.accumulator.messages_value_decremented,
                        my_balance,
                    ) {
                        return Err(generated::types::Errno::Inbalance.into());
                    }
                }

                let Some((matched_node, matched_params)) = self
                    .context
                    .data
                    .accumulator
                    .message_fee_allocation
                    .iter_mut()
                    .find_map(|node| {
                        node.matches_internal(on, calldata::Address::zero(), abi::CallKey::DEPLOY)
                            .map(|params| (node, params))
                    })
                else {
                    log_warn!(
                        recipient = calldata::Address::zero(),
                        call_key:? = abi::CallKey::DEPLOY,
                        on:? = on;
                        "no matching node for message fee allocation"
                    );

                    return Err(oom_trap(abi::consts::VmError::oom().fees().internal()));
                };

                let code_length = code.len() as u64;
                let mut enc = calldata::Encoder::new(calldata::CounterWriter(0));
                calldata::codec::Encode::encode(&calldata, &mut enc).unwrap_or_else(|e| match e {});
                let calldata_length = enc.into_inner().0;

                let fee_params = (*matched_params).clone();
                let subtree = bytes::Bytes::from(domain::fees::MessageAllocationNode::abi_encode(
                    &matched_node.children,
                ));

                let fees = consume_message_fee_internal(
                    &self.context.data.supervisor.shared_data,
                    matched_node,
                    matched_params,
                    on,
                    ConsumeInternalArgs {
                        is_deploy: true,
                        calldata_length,
                        code_length,
                        subtree_length: subtree.len() as u64,
                    },
                )
                .await?;

                self.context.data.accumulator.emissions.push(
                    domain::ExecutionEmission::DeployContract {
                        calldata,
                        code,
                        value,
                        on,
                        salt_nonce,
                        message_fee: fees.message_fee.sum(),
                        receipt_fee: fees.receipt_fee.sum(),
                        fee_params,
                        subtree,
                    },
                );

                self.context.data.accumulator.messages_value_decremented = self
                    .context
                    .data
                    .accumulator
                    .messages_value_decremented
                    .saturating_add(value);

                Ok(file_fd_none())
            }
            gl_call::Message::WebRender(render_payload) => {
                let is_det = self.context.data.conf.is_deterministic;
                if is_det {
                    return Err(generated::types::Errno::Forbidden.into());
                }

                let space_left = self
                    .context
                    .data
                    .supervisor
                    .limiter
                    .get(is_det)
                    .get_remaining_memory();

                if space_left < abi::consts::top_limits::WEB_RENDER_MIN_SPACE {
                    log_warn!(space_left = space_left; "not enough memory for web render");
                    return Err(generated::types::Error::trap(crate::anyhow_to_wasmtime(
                        rt::errors::Error::vm(abi::consts::VmError::oom().val()).into(),
                    )));
                }

                let space_left_with_overhead = (space_left as u64 * 3 / 4) as u32;

                let web = self.context.data.supervisor.modules.web.clone();
                let task = taskify(async move {
                    web.send::<genvm_modules_interfaces::web::RenderAnswer, _>(
                        genvm_modules_interfaces::web::Message::Render(
                            gl_call_to_mi::render_payload(render_payload),
                            space_left_with_overhead,
                        ),
                    )
                    .await
                })
                .await
                .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

                self.place_content(vfs::FileContents::from(bytes::Bytes::from(task)))
            }
            gl_call::Message::WebRequest(request_payload) => {
                let is_det = self.context.data.conf.is_deterministic;
                if is_det {
                    return Err(generated::types::Errno::Forbidden.into());
                }

                let space_left = self
                    .context
                    .data
                    .supervisor
                    .limiter
                    .get(is_det)
                    .get_remaining_memory();

                if space_left < abi::consts::top_limits::WEB_REQUEST_MIN_SPACE {
                    log_warn!(space_left = space_left; "not enough memory for web request");
                    return Err(generated::types::Error::trap(crate::anyhow_to_wasmtime(
                        rt::errors::Error::vm(abi::consts::VmError::oom().val()).into(),
                    )));
                }

                let space_left_with_overhead = (space_left as u64 * 3 / 4) as u32;

                let web = self.context.data.supervisor.modules.web.clone();
                let task = taskify(async move {
                    web.send::<genvm_modules_interfaces::web::RenderAnswer, _>(
                        genvm_modules_interfaces::web::Message::Request(
                            gl_call_to_mi::request_payload(request_payload),
                            space_left_with_overhead,
                        ),
                    )
                    .await
                })
                .await
                .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

                self.place_content(vfs::FileContents::from(bytes::Bytes::from(task)))
            }
            gl_call::Message::ExecPrompt(prompt_payload) => {
                if self.context.data.conf.is_deterministic {
                    return Err(generated::types::Errno::Forbidden.into());
                }

                if prompt_payload.images.len() > 2 {
                    return Err(generated::types::Errno::Inval.into());
                }

                let host_remaining_fuel = self
                    .context
                    .data
                    .supervisor
                    .host
                    .lock_for(host::host_fns::Methods::RemainingFuelAsGen)
                    .await
                    .remaining_fuel_as_gen()
                    .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;
                let remaining_fuel_as_gen = self
                    .context
                    .data
                    .supervisor
                    .shared_data
                    .remaining_det_fuel(host_remaining_fuel)
                    .await;

                let sup = self.context.data.supervisor.clone();
                let response_format = prompt_payload.response_format;

                let task = taskify(async move {
                    let result = sup
                        .modules
                        .llm
                        .send::<genvm_modules_interfaces::llm::PromptAnswer, _>(
                            genvm_modules_interfaces::llm::Message::Prompt {
                                payload: gl_call_to_mi::prompt_payload(prompt_payload),
                                remaining_fuel_as_gen,
                            },
                        )
                        .await?;

                    let result = match result {
                        Ok(r) => r,
                        Err(e) => {
                            return Ok(Err(e));
                        }
                    };

                    sup.host
                        .lock_for(host::host_fns::Methods::ConsumeFuel)
                        .await
                        .consume_fuel(result.consumed_gen)?;
                    sup.shared_data.consume_det_fuel(result.consumed_gen).await;

                    if result.consumed_gen == primitive_types::U256::MAX {
                        return Err(rt::errors::Error::vm(abi::consts::VmError::timeout()).into());
                    }

                    {
                        let mut acc = sup.shared_data.llm_consumption.lock().await;
                        *acc = acc.saturating_add(result.consumed_gen);
                    }

                    // v0.2 ABI: exec_prompt(response_format='json') must hand the
                    // contract a calldata map, not the raw JSON string. Re-encode here
                    // so JSON integers stay calldata integers and non-integer floats
                    // become calldata strings (v0.2 calldata has no native float).
                    let result = match (response_format, result.data) {
                        (
                            gl_call::llm_iface::OutputFormat::JSON,
                            genvm_modules_interfaces::llm::PromptAnswerData::Text(json),
                        ) => {
                            let parsed: serde_json::Map<String, serde_json::Value> =
                                serde_json::from_str(&json).map_err(|e| {
                                    anyhow::anyhow!("parsing json answer {json:?}: {e}")
                                })?;
                            genvm_modules_interfaces::llm::PromptAnswerData::Object(
                                crate::wasi::json_to_calldata::json_map_to_calldata(parsed),
                            )
                        }
                        (_, other) => other,
                    };

                    Ok(Ok(result))
                })
                .await
                .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

                self.place_content(vfs::FileContents::from(bytes::Bytes::from(task)))
            }
            gl_call::Message::ExecPromptTemplate(prompt_template_payload) => {
                if self.context.data.conf.is_deterministic {
                    return Err(generated::types::Errno::Forbidden.into());
                }

                let expect_bool = !matches!(
                    &prompt_template_payload,
                    gl_call::llm_iface::PromptTemplatePayload::EqNonComparativeLeader(_)
                );

                // Get remaining fuel from host
                let host_remaining_fuel = self
                    .context
                    .data
                    .supervisor
                    .host
                    .lock_for(host::host_fns::Methods::RemainingFuelAsGen)
                    .await
                    .remaining_fuel_as_gen()
                    .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;
                let remaining_fuel_as_gen = self
                    .context
                    .data
                    .supervisor
                    .shared_data
                    .remaining_det_fuel(host_remaining_fuel)
                    .await;

                let sup = self.context.data.supervisor.clone();
                let task = taskify(async move {
                    let answer = sup
                        .modules
                        .llm
                        .send::<genvm_modules_interfaces::llm::PromptAnswer, _>(
                            genvm_modules_interfaces::llm::Message::PromptTemplate {
                                payload: gl_call_to_mi::prompt_template_payload(
                                    prompt_template_payload,
                                ),
                                remaining_fuel_as_gen,
                            },
                        )
                        .await?;
                    use genvm_modules_interfaces::llm::{PromptAnswer, PromptAnswerData};

                    if let Ok(PromptAnswer { consumed_gen, .. }) = &answer {
                        sup.host
                            .lock_for(host::host_fns::Methods::ConsumeFuel)
                            .await
                            .consume_fuel(*consumed_gen)?;
                        sup.shared_data.consume_det_fuel(*consumed_gen).await;
                        if *consumed_gen == primitive_types::U256::MAX {
                            return Err(
                                rt::errors::Error::vm(abi::consts::VmError::timeout()).into()
                            );
                        }

                        {
                            let mut acc = sup.shared_data.llm_consumption.lock().await;
                            *acc = acc.saturating_add(*consumed_gen);
                        }
                    }

                    match (expect_bool, answer) {
                        (_, Err(e)) => Ok(Err(e)),
                        (
                            true,
                            Ok(PromptAnswer {
                                data: PromptAnswerData::Bool(answer),
                                ..
                            }),
                        ) => Ok(Ok(PromptAnswerData::Bool(answer))),
                        (
                            false,
                            Ok(PromptAnswer {
                                data: PromptAnswerData::Text(answer),
                                ..
                            }),
                        ) => Ok(Ok(PromptAnswerData::Text(answer))),
                        (_, Ok(_)) => Err(anyhow::anyhow!("unmatched result")),
                    }
                })
                .await
                .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

                self.place_content(vfs::FileContents::from(bytes::Bytes::from(task)))
            }
            // v0.2 ABI: the runner signals a user error by sending a `Rollback`
            // gl_call message carrying a plain string (see gl_call.py `rollback`).
            gl_call::Message::Rollback(msg) => Err(generated::types::Error::trap(
                crate::anyhow_to_wasmtime(rt::errors::Error::user(msg).into()),
            )),
            gl_call::Message::Return(value) => Err(generated::types::Error::trap(
                crate::anyhow_to_wasmtime(ContractReturn(value).into()),
            )),
            gl_call::Message::RunNondet {
                data_leader,
                data_validator,
            } => self.run_nondet(data_leader, data_validator).await,
            gl_call::Message::Sandbox {
                data,
                allow_write_ops,
            } => self.sandbox(data, allow_write_ops).await,
            gl_call::Message::RegisterRunner { code } => self.register_runner(code).await,
            gl_call::Message::MapFile {
                runner,
                path_in_runner,
                path_in_vfs,
            } => self.map_file(runner, path_in_runner, path_in_vfs).await,
            gl_call::Message::Trace(message) => self.gl_call_trace(message).await,
            gl_call::Message::Yield => Ok(file_fd_none()),
            gl_call::Message::GetTimestamp => self.gl_call_get_timestamp().await,
        }
    }

    async fn storage_read(
        &mut self,
        mem: &mut wiggle::GuestMemory<'_>,
        slot: wiggle::GuestPtr<u8>,
        index: u32,
        buf: wiggle::GuestPtr<u8>,
        buf_len: u32,
    ) -> Result<(), generated::types::Error> {
        let buf = buf.as_array(buf_len);

        if !self.context.data.conf.can_read_storage {
            return Err(generated::types::Errno::Forbidden.into());
        }

        if index.checked_add(buf_len).is_none() {
            return Err(generated::types::Errno::Inval.into());
        }

        mem.bounds_check(buf)?;

        let account = self.context.data.message_data.message.contract_address;

        let slot = SlotID::read_from_mem(mem, slot)?;
        let mem_size = buf_len as usize;

        let mut vec_buf = Vec::new();
        let (should_copy, vec) = if let Some(buf) = mem.as_slice_mut(buf)? {
            (false, buf)
        } else {
            vec_buf.resize(mem_size, 0);
            (true, vec_buf.as_mut_slice())
        };

        if self.context.data.conf.state_mode == public_abi::StorageType::Default {
            self.context
                .data
                .storage
                .read(slot, index, vec)
                .await
                .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e.into())))?;
        } else {
            self.context
                .data
                .supervisor
                .host
                .lock_for(host::host_fns::Methods::StorageRead)
                .await
                .storage_read(self.context.data.conf.state_mode, account, slot, index, vec)
                .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;
        }

        if should_copy {
            mem.copy_from_slice(&vec_buf, buf)?;
        }

        Ok(())
    }

    async fn storage_write(
        &mut self,
        mem: &mut wiggle::GuestMemory<'_>,
        slot: wiggle::GuestPtr<u8>,
        index: u32,
        buf: wiggle::GuestPtr<u8>,
        buf_len: u32,
    ) -> Result<(), generated::types::Error> {
        let buf = buf.as_array(buf_len);

        if !self.context.data.conf.is_deterministic {
            return Err(generated::types::Errno::Forbidden.into());
        }
        if !self.context.data.conf.can_write_storage {
            return Err(generated::types::Errno::Forbidden.into());
        }

        if index.checked_add(buf_len).is_none() {
            return Err(generated::types::Errno::Inval.into());
        }

        mem.bounds_check(buf)?;

        let slot = SlotID::read_from_mem(mem, slot)?;

        if self.context.data.supervisor.locked_slots.contains(slot) {
            return Err(generated::types::Errno::Forbidden.into());
        }

        let ptr = mem.as_cow(buf)?;

        self.context
            .data
            .storage
            .write(slot, index, &ptr)
            .await
            .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e.into())))
    }

    async fn get_balance(
        &mut self,
        mem: &mut wiggle::GuestMemory<'_>,
        account: wiggle::GuestPtr<u8>,
        result: wiggle::GuestPtr<u8>,
    ) -> Result<(), generated::types::Error> {
        let address = read_addr_from_mem(mem, account)?;

        self.context
            .get_balance_impl_wasi(mem, address, result, false)
            .await
    }

    async fn get_self_balance(
        &mut self,
        mem: &mut wiggle::GuestMemory<'_>,
        result: wiggle::GuestPtr<u8>,
    ) -> Result<(), generated::types::Error> {
        if !self.context.data.conf.is_deterministic {
            return Err(generated::types::Errno::Forbidden.into());
        }

        self.context
            .get_balance_impl_wasi(
                mem,
                self.context.data.message_data.message.contract_address,
                result,
                true,
            )
            .await
    }
}

impl Context {
    async fn get_balance_impl_wasi(
        &mut self,
        mem: &mut wiggle::GuestMemory<'_>,
        address: calldata::Address,
        result: wiggle::GuestPtr<u8>,
        is_self: bool,
    ) -> Result<(), generated::types::Error> {
        let mut res = self.get_balance_impl(address).await?;

        if is_self && self.data.conf.is_main() {
            let messages_decremented = self.data.accumulator.messages_value_decremented;

            res -= messages_decremented;
        }

        let res = res.to_little_endian();
        mem.copy_from_slice(&res, result.as_array(32))?;

        Ok(())
    }

    pub async fn get_balance_impl(
        &mut self,
        address: calldata::Address,
    ) -> Result<primitive_types::U256, generated::types::Error> {
        if let Some(res) = self.data.supervisor.balances.get(&address) {
            return Ok(*res);
        }

        let res = self
            .data
            .supervisor
            .host
            .lock_for(host::host_fns::Methods::GetBalance)
            .await
            .get_balance(address)
            .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

        let _ = self.data.supervisor.balances.insert(address, res);

        Ok(res)
    }

    pub fn log(&self) -> calldata::Value {
        let msg = calldata::to_value(&self.data.message_data);
        let conf = calldata::to_value(&self.data.conf);

        calldata::Value::Map(BTreeMap::from([
            ("config".to_owned(), conf),
            ("message".to_owned(), msg),
        ]))
    }
}

impl ContextVFS<'_> {
    async fn gl_call_trace(
        &mut self,
        msg: gl_call::TracePayload,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        match msg {
            gl_call::TracePayload::Message(text) => {
                // Tracing is a debug-only aid; skip the timing and logging work
                // entirely when not in debug mode.
                if !self
                    .context
                    .data
                    .supervisor
                    .shared_data
                    .debug_mode
                    .allows_tracing()
                {
                    return Ok(file_fd_none());
                }

                let now = std::time::Instant::now();
                let since_prev = now.duration_since(self.context.prev_time);
                self.context.prev_time = now;

                log_info!(
                    message = text,
                    elapsed:? = now.duration_since(self.context.start_time),
                    since_last_trace:? = since_prev;
                    "trace"
                );

                Ok(file_fd_none())
            }
            gl_call::TracePayload::RuntimeMicroSec => {
                let elapsed_micros = if self.context.data.conf.is_deterministic
                    && !self
                        .context
                        .data
                        .supervisor
                        .shared_data
                        .debug_mode
                        .allows_nondeterminism()
                {
                    0u64
                } else {
                    let elapsed = std::time::Instant::now().duration_since(self.context.start_time);
                    elapsed.as_micros() as u64
                };

                let data = calldata::encode(&calldata::Value::Number(num_bigint::BigInt::from(
                    elapsed_micros,
                )));
                self.place_content(vfs::FileContents::from(bytes::Bytes::from(data)))
            }
        }
    }

    async fn gl_call_get_timestamp(
        &mut self,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        let timestamp = if self.context.data.conf.is_deterministic {
            self.context.data.message_data.message.datetime.timestamp()
        } else {
            chrono::Utc::now().timestamp()
        };

        let data = calldata::encode(&calldata::Value::Number(num_bigint::BigInt::from(
            timestamp,
        )));
        self.place_content(vfs::FileContents::from(bytes::Bytes::from(data)))
    }

    async fn register_runner(
        &mut self,
        code: bytes::Bytes,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        if !self.context.data.conf.is_deterministic {
            return Err(generated::types::Errno::Forbidden.into());
        }
        if !self.context.data.conf.can_register_runners {
            return Err(generated::types::Errno::Forbidden.into());
        }

        let is_det = self.context.data.conf.is_deterministic;
        let supervisor = self.context.data.supervisor.clone();
        let hash = crate::runners::custom_runner_hash(&code);
        let id = supervisor
            .register_custom_runner(code, supervisor.limiter.get(is_det))
            .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

        // Scope the runner to this execution (and its deterministic children) so
        // it cannot be resolved from an unrelated scope (e.g. a nondet sub-VM).
        self.context.data.accumulator.custom_runners =
            self.context.data.accumulator.custom_runners.insert(hash);

        let data = calldata::encode(&calldata::Value::Str(id.as_str().to_owned()));
        self.place_content(vfs::FileContents::from(bytes::Bytes::from(data)))
    }

    async fn map_file(
        &mut self,
        runner: String,
        path_in_runner: String,
        path_in_vfs: String,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        // Resolving a `chain:` runner reads another contract's storage, so this
        // is gated on the same permission as `storage_read` to avoid becoming a
        // read-storage bypass.
        if !self.context.data.conf.can_read_storage {
            return Err(generated::types::Errno::Forbidden.into());
        }

        let supervisor = self.context.data.supervisor.clone();
        let is_det = self.context.data.conf.is_deterministic;
        let limiter = supervisor.limiter.get(is_det);
        let topmost_runner_id = self.context.data.conf.topmost_runner_id.clone();
        let available_custom = self.context.data.accumulator.custom_runners.clone();

        let runner =
            rt::supervisor::actions::resolve_runner_id(&supervisor, &topmost_runner_id, &runner)
                .await
                .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

        rt::supervisor::actions::map_runner_file(
            &supervisor,
            self.preview1,
            limiter,
            runner,
            &path_in_runner,
            &path_in_vfs,
            &available_custom,
        )
        .await
        .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

        Ok(file_fd_none())
    }

    async fn run_nondet(
        &mut self,
        data_leader: bytes::Bytes,
        data_validator: bytes::Bytes,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        if !self.context.data.conf.can_spawn_nondet {
            return Err(generated::types::Errno::Forbidden.into());
        }

        let call_no = self
            .context
            .data
            .supervisor
            .nondet_call_no
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        if call_no >= public_abi::top_limits::NONDET_BLOCKS {
            return Err(generated::types::Error::trap(crate::anyhow_to_wasmtime(
                rt::errors::Error::vm(abi::consts::VmError::oom().val()).into(),
            )));
        }

        let leaders_res_bytes = self
            .context
            .data
            .supervisor
            .get_leader_nondet_result(call_no);

        let leaders_res = match leaders_res_bytes {
            None if self.context.data.supervisor.is_leader() => None,
            None => {
                return Err(generated::types::Error::trap(crate::anyhow_to_wasmtime(
                    rt::errors::Error::vm(abi::consts::VmError::absent()).into(),
                )));
            }
            Some(data) if data.is_empty() => {
                // A zero-length leader result is malformed: it carries no result
                // code byte to dispatch on. It is contract-triggerable, so it must
                // not panic via out-of-bounds indexing. In `sync` mode there are no
                // validators to disagree, so surface a canonical VMError; otherwise
                // record a non-deterministic disagreement and return that VMError to
                // the validator's contract.
                if !self.context.data.supervisor.shared_data.is_sync {
                    self.context
                        .data
                        .supervisor
                        .mark_nondet_disagreement(call_no);
                }

                let result = rt::vm::RunOk::VMError(abi::consts::VmError::absent(), None);

                consume_nondet_output(
                    &self.context.data.supervisor.shared_data,
                    result.as_bytes().len() as u64,
                )
                .await?;

                return self.set_vm_run_result(result).map(|x| x.0);
            }
            Some(data) => {
                use crate::public_abi::ResultCode;
                let rest = &data[1..];
                let res = match data[0] {
                    x if x == ResultCode::Return as u8 => {
                        rt::vm::RunOk::Return(calldata::unparsed::Maybe::Checked(
                            calldata::unparsed::Raw(bytes::Bytes::copy_from_slice(rest)),
                        ))
                    }
                    x if x == ResultCode::UserError as u8 => {
                        // v0.2 ABI: the UserError payload is a raw UTF-8 string.
                        let msg = std::str::from_utf8(rest).map_err(|e| {
                            generated::types::Error::trap(crate::anyhow_to_wasmtime(
                                anyhow::anyhow!(e),
                            ))
                        })?;
                        rt::vm::RunOk::UserError(msg.to_owned())
                    }
                    x if x == ResultCode::VmError as u8 => {
                        let code = std::str::from_utf8(rest).map_err(|e| {
                            generated::types::Error::trap(crate::anyhow_to_wasmtime(
                                anyhow::anyhow!(e),
                            ))
                        })?;
                        rt::vm::RunOk::VMError(
                            public_abi::VmError(std::borrow::Cow::Owned(code.to_owned())),
                            None,
                        )
                    }
                    x => {
                        return Err(generated::types::Error::trap(crate::anyhow_to_wasmtime(
                            anyhow::anyhow!("invalid leader result code: {}", x),
                        )));
                    }
                };
                Some(res)
            }
        };

        let result_to_return = if self.context.data.supervisor.shared_data.is_sync {
            match leaders_res {
                None => {
                    return Err(generated::types::Error::trap(crate::anyhow_to_wasmtime(
                        anyhow::anyhow!("absent leader result in sync mode, call_no: {}", call_no),
                    )))
                }
                Some(v) => v,
            }
        } else {
            let storage_checkpoint = self.context.data.storage.clone();

            let message_data = match &leaders_res {
                None => self.context.data.message_data.fork_leader(
                    public_abi::EntryKind::ConsensusStage,
                    data_leader,
                    None,
                ),
                Some(leaders_res) => {
                    let dup = match leaders_res {
                        rt::vm::RunOk::Return(items) => rt::vm::RunOk::Return(items.clone()),
                        rt::vm::RunOk::UserError(msg) => rt::vm::RunOk::UserError(msg.clone()),
                        rt::vm::RunOk::VMError(msg, _) => rt::vm::RunOk::VMError(msg.clone(), None),
                    };
                    self.context.data.message_data.fork_leader(
                        public_abi::EntryKind::ConsensusStage,
                        data_validator,
                        Some(dup),
                    )
                }
            };

            let supervisor = self.context.data.supervisor.clone();

            let fake_accum = VMDataAccumulator {
                data_fees_limit: self.context.data.accumulator.data_fees_limit.clone(),
                messages_value_decremented: self
                    .context
                    .data
                    .accumulator
                    .messages_value_decremented,
                emissions: Vec::new(),
                message_fee_allocation: Vec::new(),
                // Nondet is an isolated execution scope: it must NOT see custom
                // runners registered by the deterministic scope.
                custom_runners: Default::default(),
            };

            let vm_data = Box::new(SingleVMData {
                remaining_recursion: self.context.data.remaining_recursion.saturating_sub(1),
                signer_address: self.context.data.signer_address,
                // Permission model: docs/website/src/spec/03-vm/02-meta-properties.rst
                conf: base::Config {
                    needs_error_fingerprint: false,
                    is_deterministic: false,
                    can_read_storage: self.context.data.conf.can_read_storage,
                    can_write_storage: false,
                    can_spawn_nondet: false,
                    can_call_others: false,
                    can_send_messages: false,
                    can_register_runners: false,
                    state_mode: public_abi::StorageType::Default,
                    topmost_runner_id: self.context.data.conf.topmost_runner_id.clone(),
                },
                message_data,
                supervisor: supervisor.clone(),
                storage: storage_checkpoint,
                accumulator: fake_accum,
                det_subvm_hashes: Default::default(), // won't be used
            });

            let task_done = Arc::new(tokio::sync::Notify::new());
            let task = rt::supervisor::NonDetVMTask {
                task: vm_data,
                call_no,
                tasks_done: task_done.clone(),
            };

            match leaders_res {
                None => {
                    let res = task
                        .run_now(&self.context.data.supervisor)
                        .await
                        .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

                    self.context
                        .data
                        .supervisor
                        .push_nondet_result(call_no, bytes::Bytes::from(res.as_bytes()))
                        .await;

                    res
                }
                Some(leaders_res) => {
                    rt::supervisor::submit_nondet_vm_task(&self.context.data.supervisor, task)
                        .await;

                    leaders_res
                }
            }
        };

        consume_nondet_output(
            &self.context.data.supervisor.shared_data,
            result_to_return.as_bytes().len() as u64,
        )
        .await?;

        self.set_vm_run_result(result_to_return).map(|x| x.0)
    }

    async fn sandbox(
        &mut self,
        data: bytes::Bytes,
        allow_write_ops: bool,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        let supervisor = self.context.data.supervisor.clone();

        // v0.2.16 sandboxes run the SAME contract (no `runner` field): the
        // sub-VM inherits the parent's topmost runner.
        let topmost_runner_id = self.context.data.conf.topmost_runner_id.clone();

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
            // Sandbox is a deterministic sub-VM: inherit the registered runners.
            custom_runners: self.context.data.accumulator.custom_runners.clone(),
        };

        std::mem::swap(&mut self.context.data.accumulator, &mut fake_my_data);

        let stolen_data = fake_my_data;

        let vm_data = Box::new(SingleVMData {
            remaining_recursion: self.context.data.remaining_recursion.saturating_sub(1),
            signer_address: self.context.data.signer_address,
            // Permission model: docs/website/src/spec/03-vm/02-meta-properties.rst
            conf: base::Config {
                needs_error_fingerprint: false,
                is_deterministic: zelf_conf.is_deterministic,
                can_read_storage: zelf_conf.can_read_storage,
                can_write_storage: zelf_conf.can_write_storage & allow_write_ops,
                can_spawn_nondet: false,
                can_call_others: false,
                can_send_messages: zelf_conf.can_send_messages & allow_write_ops,
                can_register_runners: false,
                state_mode: zelf_conf.state_mode,
                topmost_runner_id,
            },
            message_data,
            supervisor: supervisor.clone(),
            storage: storage_checkpoint,
            accumulator: stolen_data,
            det_subvm_hashes: Default::default(),
        });

        let my_res = rt::spawn_apply_run(&supervisor, vm_data)
            .await
            .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

        if self.context.data.conf.is_deterministic {
            let hash = my_res.small_hash();
            self.context.data.det_subvm_hashes.update(&hash);
        }

        self.context.data.accumulator = my_res.vm_data.accumulator;
        self.context.data.storage = my_res.vm_data.storage;

        let data: Vec<u8> = my_res.run_ok.as_bytes();
        self.place_content(vfs::FileContents::from(bytes::Bytes::from(data)))
    }
}

/// Conversions from sdk-rs `gl_call` payload types to the self-contained
/// `genvm_modules_interfaces` payload types.
///
/// These types are structurally identical but distinct: `genvm-modules-interfaces`
/// no longer depends on `sdk-rs`, so it carries its own copies. The orphan rule
/// prevents `From`/`Into` impls here (both types are foreign to this crate), so
/// free functions bridge the executor<->module boundary instead.
mod gl_call_to_mi {
    use genlayer_sdk::abi::gl_call::{llm_iface, web_iface};
    use genvm_modules_interfaces::{llm as mi_llm, web as mi_web};

    pub fn render_mode(m: web_iface::RenderMode) -> mi_web::RenderMode {
        match m {
            web_iface::RenderMode::Text => mi_web::RenderMode::Text,
            web_iface::RenderMode::HTML => mi_web::RenderMode::HTML,
            web_iface::RenderMode::Screenshot => mi_web::RenderMode::Screenshot,
        }
    }

    pub fn wait_after_loaded(w: web_iface::WaitAfterLoaded) -> mi_web::WaitAfterLoaded {
        match w {
            web_iface::WaitAfterLoaded::Seconds(s) => mi_web::WaitAfterLoaded::Seconds(s),
            web_iface::WaitAfterLoaded::Millis(ms) => mi_web::WaitAfterLoaded::Millis(ms),
        }
    }

    pub fn render_payload(p: web_iface::RenderPayload) -> mi_web::RenderPayload {
        mi_web::RenderPayload {
            mode: render_mode(p.mode),
            url: p.url,
            wait_after_loaded: wait_after_loaded(p.wait_after_loaded),
        }
    }

    pub fn request_method(m: web_iface::RequestMethod) -> mi_web::RequestMethod {
        match m {
            web_iface::RequestMethod::GET => mi_web::RequestMethod::GET,
            web_iface::RequestMethod::POST => mi_web::RequestMethod::POST,
            web_iface::RequestMethod::HEAD => mi_web::RequestMethod::HEAD,
            web_iface::RequestMethod::PUT => mi_web::RequestMethod::PUT,
            web_iface::RequestMethod::DELETE => mi_web::RequestMethod::DELETE,
            web_iface::RequestMethod::OPTIONS => mi_web::RequestMethod::OPTIONS,
            web_iface::RequestMethod::PATCH => mi_web::RequestMethod::PATCH,
        }
    }

    pub fn request_payload(p: web_iface::RequestPayload) -> mi_web::RequestPayload {
        mi_web::RequestPayload {
            method: request_method(p.method),
            url: p.url,
            headers: p.headers,
            body: p.body,
            sign: p.sign,
        }
    }

    pub fn output_format(f: llm_iface::OutputFormat) -> mi_llm::OutputFormat {
        match f {
            llm_iface::OutputFormat::Text => mi_llm::OutputFormat::Text,
            llm_iface::OutputFormat::JSON => mi_llm::OutputFormat::JSON,
        }
    }

    pub fn prompt_payload(p: llm_iface::PromptPayload) -> mi_llm::PromptPayload {
        mi_llm::PromptPayload {
            response_format: output_format(p.response_format),
            prompt: p.prompt,
            images: p.images,
        }
    }

    pub fn prompt_template_payload(
        p: llm_iface::PromptTemplatePayload,
    ) -> mi_llm::PromptTemplatePayload {
        match p {
            llm_iface::PromptTemplatePayload::EqComparative(x) => {
                mi_llm::PromptTemplatePayload::EqComparative(mi_llm::PromptEqComparativePayload {
                    leader_answer: x.leader_answer,
                    validator_answer: x.validator_answer,
                    principle: x.principle,
                })
            }
            llm_iface::PromptTemplatePayload::EqNonComparativeValidator(x) => {
                mi_llm::PromptTemplatePayload::EqNonComparativeValidator(
                    mi_llm::PromptEqNonComparativeValidatorPayload {
                        task: x.task,
                        criteria: x.criteria,
                        input: x.input,
                        output: x.output,
                    },
                )
            }
            llm_iface::PromptTemplatePayload::EqNonComparativeLeader(x) => {
                mi_llm::PromptTemplatePayload::EqNonComparativeLeader(
                    mi_llm::PromptEqNonComparativeLeaderPayload {
                        task: x.task,
                        criteria: x.criteria,
                        input: x.input,
                    },
                )
            }
        }
    }
}
