use std::collections::BTreeMap;
use std::sync::Arc;

use genvm_common::internal_constants::{memory_limiter_consts, top_limits};
use genvm_common::sync::DArc;
use genvm_common::*;

use genvm_modules_interfaces::GenericValue;
use wiggle::GuestError;

use crate::host::{self, SlotID};
use crate::int_traits::*;
use crate::{anyhow_to_wasmtime, calldata, domain, public_abi, rt, wasi};

pub use genlayer_sdk::abi::entry::ExtendedMessage;
use genlayer_sdk::abi::{self, gl_call};

use super::{base, vfs};

mod message;
mod run;
#[cfg(test)]
mod tests;

// Leader-result parsing/stripping. Pure functions over untrusted bytes,
// re-exported (rather than making `run` public) so the fuzz targets can reach
// them.
pub use run::{parse_leader_result, strip_vm_error_detail};

fn default_entry_stage_data() -> calldata::Value {
    calldata::Value::Null
}

fn internal_trap(error: rt::errors::Error) -> generated::types::Error {
    generated::types::Error::trap(anyhow_to_wasmtime(error.into()))
}

fn reserve_permanent(
    limiter: &rt::memlimiter::Limiter,
    amount: u64,
    allocation: &'static str,
) -> Result<rt::memlimiter::PermanentAllocation, generated::types::Error> {
    limiter.reserve_permanent(amount).ok_or_else(|| {
        internal_trap(rt::errors::Error::wrap(
            abi::consts::VmError::out_of().memory().val(),
            anyhow::anyhow!("retaining {amount} bytes for {allocation}"),
        ))
    })
}

fn emission_allocation_size(payload_sizes: &[u64]) -> u64 {
    payload_sizes
        .iter()
        .copied()
        .try_fold(
            memory_limiter_consts::EXECUTION_EMISSION_BASE_SIZE.into(),
            u64::checked_add,
        )
        .unwrap_or(u64::MAX)
}

/// Extension methods for ExtendedMessage specific to the executor
pub trait ExtendedMessageExt {
    fn fork_leader(
        &self,
        entry_kind: public_abi::EntryKind,
        entry_data: bytes::Bytes,
        entry_leader_data: Option<rt::vm::ContractOutcome>,
    ) -> ExtendedMessage;

    fn fork(&self, entry_kind: public_abi::EntryKind, entry_data: bytes::Bytes) -> ExtendedMessage;
}

impl ExtendedMessageExt for ExtendedMessage {
    fn fork_leader(
        &self,
        entry_kind: public_abi::EntryKind,
        entry_data: bytes::Bytes,
        entry_leader_data: Option<rt::vm::ContractOutcome>,
    ) -> ExtendedMessage {
        use genlayer_sdk::abi::entry::MessageData;

        let entry_leader_data = match entry_leader_data {
            None => default_entry_stage_data(),
            Some(entry_leader_data) => calldata::Value::Map(BTreeMap::from([(
                "leader_result".into(),
                calldata::Value::Bytes(entry_leader_data.encode().into_bytes().to_vec()),
            )])),
        };

        ExtendedMessage {
            message: MessageData {
                contract_address: self.message.contract_address,
                sender_address: self.message.sender_address,
                origin_address: self.message.origin_address,
                signer_address: self.message.signer_address,
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
    pub mode: public_abi::StorageView,
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
    pub message_fee_allocation: Vec<genvm_modules_interfaces::fees::MessageAllocationNode>,
    pub message_fee_allocation_consumed: Vec<primitive_types::U256>,
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
    pub limiter: rt::memlimiter::Limiter,
    /// Recursion budget left to this VM and everything it spawns, inherited
    /// from the chain root rather than counted up from zero, so a chain that
    /// crosses executor lines keeps the bound its root minted.
    pub remaining_recursion: u32,
    pub spawn_kind: String,
    pub message_data: ExtendedMessage,
    pub supervisor: Arc<rt::supervisor::Supervisor>,
    pub storage: rt::vm::storage::Storage<StorageHostHolder>,
    pub accumulator: VMDataAccumulator,
    pub det_subvm_hashes: sha3::Sha3_256,
    /// `custom:` runner pins granted to this VM by its parent, carried from the
    /// spawning gl_call (so the content survives the parent's death for a queued
    /// nondet VM). The child's spawn performs a load action for each, charging its
    /// own limiter, before its main runner loads. Empty for the
    /// root VM. Drained at spawn.
    pub granted_custom: Vec<crate::runners::cache::ArchivePin>,
}

impl SingleVMData {
    /// How deep this VM sits, for logs and recorded actions only. Exact while
    /// the chain stays on one line; an approximation once it crosses into a
    /// line whose own limit differs.
    pub fn depth(&self) -> u32 {
        top_limits::VM_RECURSION.saturating_sub(self.remaining_recursion)
    }
}

pub struct Context {
    pub data: SingleVMData,

    /// This VM's **loaded set**: the charge-dedup set and pin holder for every
    /// runner it has loaded. A sibling of `data` (not inside its
    /// accumulator), so it does not round-trip through sandbox children and is
    /// dropped when this VM's store is torn down.
    pub loaded: crate::runners::cache::LoadedSet,

    /// A handle to this VM's own memory limiter. Runtime load actions
    /// (`RegisterRunner`, `MapFile`) charge it directly, since a gl_call handler
    /// cannot otherwise reach the store limiter.
    pub limiter: rt::memlimiter::Limiter,

    pub start_time: std::time::Instant,
    pub prev_time: std::time::Instant,
}

pub struct ContextVFS<'a> {
    pub(super) vfs: &'a mut vfs::VFS,
    pub(super) preview1: &'a mut super::preview1::Context,
    pub(super) context: &'a mut Context,
}

async fn spawn_sub_vm(
    supervisor: Arc<rt::supervisor::Supervisor>,
    vm_data: Box<SingleVMData>,
) -> anyhow::Result<rt::vm::RunResult> {
    tokio::task::spawn(async move { rt::spawn_apply_run(&supervisor, vm_data).await })
        .await
        .map_err(|e| anyhow::anyhow!("sub-VM task failed to join: {e}"))?
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
    let cow = mem.as_cow(addr.as_array(calldata::ADDRESS_SIZE.into_int_downcast_panicking()))?;
    let mut ret = calldata::Address::zero();
    ret.ref_mut().copy_from_slice(&cow);
    Ok(ret)
}

impl SlotID {
    fn read_from_mem(
        mem: &mut wiggle::GuestMemory<'_>,
        addr: wiggle::GuestPtr<u8>,
    ) -> Result<Self, generated::types::Error> {
        let cow = mem.as_cow(addr.as_array(SlotID::len().into_int_downcast_panicking()))?;
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
    pub fn new(data: Box<SingleVMData>, limiter: rt::memlimiter::Limiter) -> Self {
        let now = std::time::Instant::now();

        // Every VM's config passes through here. Invariant: only a `Default`-state
        // VM reads through its local storage cache (read-your-writes); a VM with a
        // non-`Default` state_mode reads bypass the cache, so it must not be able to
        // write (otherwise its writes would hit the cache but never be read back).
        debug_assert!(
            data.conf.execution.state_mode == public_abi::StorageView::Default
                || !data.conf.permissions.write_storage,
            "a VM with state_mode != Default must not have can_write_storage"
        );

        Self {
            data: *data,
            loaded: Default::default(),
            limiter,
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

    /// Hand a sub-VM's outcome back to the caller. `catch_vm_error` decides
    /// whether an ordinary VM error becomes a value the caller reads or an
    /// error it inherits; a fatal one is never catchable either way.
    fn publish_sub_vm_result(
        &mut self,
        data: rt::vm::RunOk,
        catch_vm_error: bool,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        let encoded = catchable_sub_vm_outcome(data, catch_vm_error)?.encode();
        self.place_content(vfs::FileContents::from(encoded.into_bytes()))
    }

    /// [`Self::publish_sub_vm_result`] for a caller that already holds the
    /// outcome's encoding; `encoded` must be exactly that.
    fn publish_sub_vm_result_encoded(
        &mut self,
        data: rt::vm::RunOk,
        encoded: bytes::Bytes,
        catch_vm_error: bool,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        catchable_sub_vm_outcome(data, catch_vm_error)?;
        self.place_content(vfs::FileContents::from(encoded))
    }
}

/// The outcome the caller may read, or the error it inherits instead
fn catchable_sub_vm_outcome(
    data: rt::vm::RunOk,
    catch_vm_error: bool,
) -> Result<rt::vm::ContractOutcome, generated::types::Error> {
    match data {
        rt::vm::RunOk::VMError(e, cause) if !catch_vm_error => Err(generated::types::Error::trap(
            crate::anyhow_to_wasmtime(rt::errors::Error::vm_cause(e, cause).into()),
        )),
        data => rt::vm::ContractOutcome::try_from(data)
            .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e.into()))),
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
        Ok(r) => Ok(encode_tagged("ok", &r)),
        Err(e) => Ok(encode_tagged("error", &e)),
    }
}

/// `{key: value}` encoded straight to bytes, without an intermediate [`calldata::Value`].
fn encode_tagged<T>(key: &str, value: &T) -> Box<[u8]>
where
    T: calldata::codec::Encode<Vec<u8>, Error = std::convert::Infallible>,
{
    let mut enc = calldata::Encoder::new(Vec::new());
    let res = (|| {
        enc.start_map(1)?;
        enc.push_map_k(key)?;
        value.encode(&mut enc)
    })();
    match res {
        Ok(()) => {}
        Err(e) => match e {},
    }
    Box::from(enc.into_inner())
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

use message::EmitInternalDeployMessageArgs;

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
            gl_call::Message::EmitExternalMessage {
                address,
                calldata,
                value,
            } => {
                self.gl_call_emit_external_message(address, calldata, value)
                    .await
            }
            gl_call::Message::ExternalCall { address, calldata } => {
                self.gl_call_external_call(address, calldata).await
            }
            gl_call::Message::CallContract {
                address,
                calldata,
                storage_view,
                catch_vm_error,
            } => {
                self.gl_call_contract(address, calldata, storage_view, catch_vm_error)
                    .await
            }
            gl_call::Message::EmitEvent { topics, blob } => {
                self.gl_call_emit_event(topics, blob).await
            }
            gl_call::Message::EmitInternalMessage {
                address,
                calldata,
                value,
                on,
                use_balance,
                fee_params,
            } => {
                self.gl_call_emit_internal_message(
                    address,
                    calldata,
                    value,
                    on,
                    use_balance,
                    fee_params,
                )
                .await
            }
            gl_call::Message::EmitInternalDeployMessage {
                calldata,
                code,
                value,
                on,
                salt_nonce,
                use_balance,
                fee_params,
            } => {
                self.gl_call_emit_internal_deploy_message(
                    calldata,
                    on,
                    fee_params,
                    EmitInternalDeployMessageArgs {
                        code,
                        value,
                        salt_nonce,
                        use_balance,
                    },
                )
                .await
            }
            gl_call::Message::WebRender(render_payload) => {
                self.gl_call_web_render(render_payload).await
            }
            gl_call::Message::WebRequest(request_payload) => {
                self.gl_call_web_request(request_payload).await
            }
            gl_call::Message::ExecPrompt(prompt_payload) => {
                self.gl_call_exec_prompt(prompt_payload).await
            }
            gl_call::Message::ExecPromptTemplate(prompt_template_payload) => {
                self.gl_call_exec_prompt_template(prompt_template_payload)
                    .await
            }
            gl_call::Message::UserError(msg) => Err(generated::types::Error::trap(
                crate::anyhow_to_wasmtime(rt::errors::Error::user(msg).into()),
            )),
            gl_call::Message::Return(value) => Err(generated::types::Error::trap(
                crate::anyhow_to_wasmtime(ContractReturn(value).into()),
            )),
            gl_call::Message::RunNondet {
                data_leader,
                data_validator,
                runner,
                custom_runners,
                catch_vm_error,
            } => {
                self.run_nondet(
                    data_leader,
                    data_validator,
                    runner,
                    custom_runners,
                    catch_vm_error,
                )
                .await
            }
            gl_call::Message::Sandbox {
                data,
                runner,
                allow_write_storage,
                allow_send_messages,
                custom_runners,
                changes_on_error: gl_call::ChangesOnError::Inherit,
            } => {
                self.sandbox(
                    data,
                    runner,
                    allow_write_storage,
                    allow_send_messages,
                    custom_runners,
                )
                .await
            }
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

        if index.checked_add(buf_len).is_none() {
            return Err(generated::types::Errno::Inval.into());
        }

        mem.bounds_check(buf)?;

        let account = self.context.data.message_data.message.contract_address;

        let slot = SlotID::read_from_mem(mem, slot)?;
        let mem_size = buf_len.into_int_comptime();

        let mut vec_buf = Vec::new();
        let (should_copy, vec) = if let Some(buf) = mem.as_slice_mut(buf)? {
            (false, buf)
        } else {
            vec_buf.resize(mem_size, 0);
            (true, vec_buf.as_mut_slice())
        };

        if self.context.data.conf.execution.state_mode == public_abi::StorageView::Default {
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
                .storage_read(
                    self.context.data.conf.execution.state_mode,
                    account,
                    slot,
                    index,
                    vec,
                )
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

        if !self.context.data.conf.permissions.deterministic {
            return Err(generated::types::Errno::Forbidden.into());
        }
        if !self.context.data.conf.permissions.write_storage {
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
        if !self.context.data.conf.permissions.deterministic {
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
            .lock_for(host::host_fns::Methods::GetBalanceGenWei)
            .await
            .get_balance_gen_wei(address)
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
    async fn gl_call_web_render(
        &mut self,
        render_payload: gl_call::web_iface::RenderPayload,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        let is_det = self.context.data.conf.permissions.deterministic;
        if is_det {
            return Err(generated::types::Errno::Forbidden.into());
        }

        let space_left = self.context.limiter.get_remaining_memory();

        if space_left < top_limits::WEB_RENDER_MIN_SPACE {
            log_warn!(space_left = space_left; "not enough memory for web render");
            return Err(generated::types::Error::trap(crate::anyhow_to_wasmtime(
                rt::errors::Error::vm(abi::consts::VmError::out_of().memory().val()).into(),
            )));
        }

        let space_left_with_overhead =
            (u64::from(space_left) * 3 / 4).into_int_downcast_panicking();

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
    async fn gl_call_web_request(
        &mut self,
        request_payload: gl_call::web_iface::RequestPayload,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        let is_det = self.context.data.conf.permissions.deterministic;
        if is_det {
            return Err(generated::types::Errno::Forbidden.into());
        }

        let space_left = self.context.limiter.get_remaining_memory();

        if space_left < top_limits::WEB_REQUEST_MIN_SPACE {
            log_warn!(space_left = space_left; "not enough memory for web request");
            return Err(generated::types::Error::trap(crate::anyhow_to_wasmtime(
                rt::errors::Error::vm(abi::consts::VmError::out_of().memory().val()).into(),
            )));
        }

        let space_left_with_overhead =
            (u64::from(space_left) * 3 / 4).into_int_downcast_panicking();

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
    async fn gl_call_exec_prompt(
        &mut self,
        prompt_payload: gl_call::llm_iface::PromptPayload,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        if self.context.data.conf.permissions.deterministic {
            return Err(generated::types::Errno::Forbidden.into());
        }

        if prompt_payload.images.len() > 2 {
            return Err(generated::types::Errno::Inval.into());
        }

        let host_remaining_time_fee_gen_wei = self
            .context
            .data
            .supervisor
            .host
            .lock_for(host::host_fns::Methods::GetRemainingTimeFeeGenWei)
            .await
            .get_remaining_time_fee_gen_wei()
            .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;
        let remaining_time_fee_gen_wei = self
            .context
            .data
            .supervisor
            .shared_data
            .remaining_det_fuel(host_remaining_time_fee_gen_wei)
            .await;

        let sup = self.context.data.supervisor.clone();

        let task = taskify(async move {
            let result = sup
                .modules
                .llm
                .send::<genvm_modules_interfaces::llm::PromptAnswer, _>(
                    genvm_modules_interfaces::llm::Message::Prompt {
                        payload: gl_call_to_mi::prompt_payload(prompt_payload),
                        remaining_time_fee_gen_wei,
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
                .lock_for(host::host_fns::Methods::ConsumeTimeFeeGenWei)
                .await
                .consume_time_fee_gen_wei(result.consumed_gen)?;
            sup.shared_data.consume_det_fuel(result.consumed_gen).await;

            if result.consumed_gen == primitive_types::U256::MAX {
                return Err(rt::errors::Error::fatal_vm(abi::consts::VmError::timeout()).into());
            }

            {
                let mut acc = sup.shared_data.llm_consumption.lock().await;
                *acc = acc.saturating_add(result.consumed_gen);
            }

            let result = result.data;

            Ok(Ok(result))
        })
        .await
        .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

        self.place_content(vfs::FileContents::from(bytes::Bytes::from(task)))
    }
    async fn gl_call_exec_prompt_template(
        &mut self,
        prompt_template_payload: gl_call::llm_iface::PromptTemplatePayload,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        if self.context.data.conf.permissions.deterministic {
            return Err(generated::types::Errno::Forbidden.into());
        }

        let expect_bool = !matches!(
            &prompt_template_payload,
            gl_call::llm_iface::PromptTemplatePayload::EqNonComparativeLeader(_)
        );

        let host_remaining_time_fee_gen_wei = self
            .context
            .data
            .supervisor
            .host
            .lock_for(host::host_fns::Methods::GetRemainingTimeFeeGenWei)
            .await
            .get_remaining_time_fee_gen_wei()
            .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;
        let remaining_time_fee_gen_wei = self
            .context
            .data
            .supervisor
            .shared_data
            .remaining_det_fuel(host_remaining_time_fee_gen_wei)
            .await;

        let sup = self.context.data.supervisor.clone();
        let task = taskify(async move {
            let answer = sup
                .modules
                .llm
                .send::<genvm_modules_interfaces::llm::PromptAnswer, _>(
                    genvm_modules_interfaces::llm::Message::PromptTemplate {
                        payload: gl_call_to_mi::prompt_template_payload(prompt_template_payload),
                        remaining_time_fee_gen_wei,
                    },
                )
                .await?;
            use genvm_modules_interfaces::llm::{PromptAnswer, PromptAnswerData};

            if let Ok(PromptAnswer { consumed_gen, .. }) = &answer {
                sup.host
                    .lock_for(host::host_fns::Methods::ConsumeTimeFeeGenWei)
                    .await
                    .consume_time_fee_gen_wei(*consumed_gen)?;
                sup.shared_data.consume_det_fuel(*consumed_gen).await;
                if *consumed_gen == primitive_types::U256::MAX {
                    return Err(rt::errors::Error::fatal_vm(abi::consts::VmError::timeout()).into());
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
            gl_call::TracePayload::RuntimeMicroseconds => {
                let elapsed_micros = if self.context.data.conf.permissions.deterministic
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
                    elapsed.as_micros().into_int_downcast_panicking()
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
        let timestamp = if self.context.data.conf.permissions.deterministic {
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
        if !self.context.data.conf.permissions.deterministic {
            return Err(generated::types::Errno::Forbidden.into());
        }
        // The load action registers the content (weak registry, dedup while
        // alive), charges `RUNNER_LOAD_COST + code.len()` to this VM before parsing, and
        // pins it into this VM's loaded set -- scoping it to this execution and its
        // deterministic children only. RegisterRunner is det-only, so
        // the load folds into the det fingerprint.
        let supervisor = self.context.data.supervisor.clone();
        let limiter = self.context.limiter.clone();
        let Context { loaded, data, .. } = &mut *self.context;
        let id = rt::supervisor::actions::register_runner_load(
            &supervisor,
            &limiter,
            loaded,
            Some(&mut data.det_subvm_hashes),
            code,
        )
        .await
        .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

        let data = calldata::encode(&calldata::Value::Str(id.as_str().to_owned()));
        self.place_content(vfs::FileContents::from(bytes::Bytes::from(data)))
    }

    async fn map_file(
        &mut self,
        runner: String,
        path_in_runner: String,
        path_in_vfs: String,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        let supervisor = self.context.data.supervisor.clone();
        let is_det = self.context.data.conf.permissions.deterministic;
        // Charge the VM's own limiter, not the long-lived root limiter. The pin
        // drops with this VM's loaded set when the store is torn down.
        let limiter = self.context.limiter.clone();
        let topmost_runner_id = self.context.data.conf.execution.topmost_runner_id.clone();

        let runner = rt::supervisor::actions::resolve_runner_id(
            &supervisor,
            &limiter,
            &topmost_runner_id,
            &runner,
        )
        .await
        .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

        // The load action pins the archive into this VM's loaded set (keeping the
        // mapped files' backing bytes resident) and charges once for it.
        let preview1 = &mut *self.preview1;
        let Context { loaded, data, .. } = &mut *self.context;
        let det_fingerprint = is_det.then_some(&mut data.det_subvm_hashes);
        rt::supervisor::actions::map_runner_file(
            &supervisor,
            preview1,
            &limiter,
            loaded,
            det_fingerprint,
            runner,
            &path_in_runner,
            &path_in_vfs,
        )
        .await
        .map_err(|e| generated::types::Error::trap(crate::anyhow_to_wasmtime(e)))?;

        Ok(file_fd_none())
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

    pub fn post_load_wait(w: web_iface::WaitAfterLoaded) -> mi_web::WaitAfterLoaded {
        match w {
            web_iface::WaitAfterLoaded::Seconds(s) => mi_web::WaitAfterLoaded::Seconds(s),
            web_iface::WaitAfterLoaded::Millis(ms) => mi_web::WaitAfterLoaded::Millis(ms),
        }
    }

    pub fn render_payload(p: web_iface::RenderPayload) -> mi_web::RenderPayload {
        mi_web::RenderPayload {
            mode: render_mode(p.mode),
            url: p.url,
            wait_after_loaded: post_load_wait(p.post_load_wait),
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
