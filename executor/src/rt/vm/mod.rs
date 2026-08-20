use crate::{domain, public_abi, rt, wasi};

use genlayer_sdk::abi;
use genvm_common::*;
use itertools::Itertools;

pub mod storage;

#[derive(Debug)]
pub enum RunOk {
    Return(calldata::unparsed::Maybe<calldata::Value>),
    UserError(calldata::unparsed::Maybe<calldata::Value>),
    VMError(abi::consts::VmError, Option<anyhow::Error>),
    FatalVMError(abi::consts::VmError, Option<anyhow::Error>),
}

#[derive(Debug)]
pub enum ContractOutcome {
    Return(calldata::unparsed::Maybe<calldata::Value>),
    UserError(calldata::unparsed::Maybe<calldata::Value>),
    VMError(abi::consts::VmError, Option<anyhow::Error>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractResultBytes(bytes::Bytes);

impl ContractResultBytes {
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub fn into_bytes(self) -> bytes::Bytes {
        self.0
    }
}

impl ContractOutcome {
    pub fn duplicate(&self) -> Self {
        match self {
            Self::Return(buf) => Self::Return(buf.clone()),
            Self::UserError(buf) => Self::UserError(buf.clone()),
            Self::VMError(e, _) => Self::VMError(e.clone(), None),
        }
    }

    pub fn encode(&self) -> ContractResultBytes {
        use crate::public_abi::ResultCode;

        let bytes = match self {
            Self::Return(buf) => {
                let encoded = calldata::encode_obj(buf);
                let mut res = Vec::with_capacity(1 + encoded.len());
                res.push(ResultCode::Return as u8);
                res.extend_from_slice(&encoded);
                res
            }
            Self::UserError(val) => {
                let mut res = vec![ResultCode::UserError as u8];
                match val {
                    calldata::unparsed::Maybe::Materialized(value) => {
                        res.extend_from_slice(&calldata::encode(value));
                    }
                    calldata::unparsed::Maybe::Checked(raw) => {
                        res.extend_from_slice(&raw.0);
                    }
                }
                res
            }
            Self::VMError(buf, _) => {
                let mut res = Vec::with_capacity(1 + buf.0.len());
                res.push(ResultCode::VmError as u8);
                res.extend_from_slice(buf.0.as_bytes());
                res
            }
        };

        ContractResultBytes(bytes.into())
    }
}

impl From<ContractOutcome> for RunOk {
    fn from(value: ContractOutcome) -> Self {
        match value {
            ContractOutcome::Return(buf) => Self::Return(buf),
            ContractOutcome::UserError(buf) => Self::UserError(buf),
            ContractOutcome::VMError(e, cause) => Self::VMError(e, cause),
        }
    }
}

impl TryFrom<RunOk> for ContractOutcome {
    type Error = rt::errors::Error;

    fn try_from(value: RunOk) -> Result<Self, Self::Error> {
        match value {
            RunOk::Return(buf) => Ok(Self::Return(buf)),
            RunOk::UserError(buf) => Ok(Self::UserError(buf)),
            RunOk::VMError(e, cause) => Ok(Self::VMError(e, cause)),
            RunOk::FatalVMError(e, cause) => Err(rt::errors::Error::fatal_vm_cause(e, cause)),
        }
    }
}

impl RunOk {
    /// Like clone, but drops the `cause` of a VM error if present
    pub fn duplicate(&self) -> Self {
        match self {
            RunOk::Return(buf) => RunOk::Return(buf.clone()),
            RunOk::UserError(buf) => RunOk::UserError(buf.clone()),
            RunOk::VMError(e, _cause) => RunOk::VMError(e.clone(), None),
            RunOk::FatalVMError(e, _cause) => RunOk::FatalVMError(e.clone(), None),
        }
    }

    pub fn into_contract_observable_bytes(self) -> rt::errors::Result<Vec<u8>> {
        ContractOutcome::try_from(self).map(|outcome| outcome.encode().into_bytes().to_vec())
    }
}

pub struct RunResult {
    pub run_ok: RunOk,
    pub backtrace: Option<rt::errors::Backtrace>,
    pub wasm_store_hashes: rt::errors::WasmStoreHashes,
    pub vm_data: Box<wasi::genlayer_sdk::SingleVMData>,
}

#[derive(Debug, Clone, genlayer_calldata::Encode)]
pub struct FullResult {
    pub kind: host_fns::ResultCode,
    pub data: calldata::unparsed::Maybe<calldata::Value>,
    pub backtrace: Option<rt::errors::Backtrace>,
    pub wasm_store_hashes: rt::errors::WasmStoreHashes,
    pub subvm_hashes: bytes::Bytes,
    pub storage_changes: Vec<storage::Delta>,

    pub emissions: Vec<domain::ExecutionEmission>,
}

/// Digest of an empty sub-VM hash accumulator, i.e. the value a run with no
/// deterministic sub-calls produces. Kept in sync with how [`RunResult`]'s
/// `det_subvm_hashes` is finalized so error/edge results hash uniformly.
pub(crate) fn empty_subvm_hashes() -> bytes::Bytes {
    bytes::Bytes::from(sha3::Digest::finalize(sha3::Sha3_256::default()).to_vec())
}

impl FullResult {
    pub fn empty_from(run_ok: RunOk) -> Self {
        Self {
            kind: match run_ok {
                RunOk::Return(_) => host_fns::ResultCode::Return,
                RunOk::UserError(_) => host_fns::ResultCode::UserError,
                RunOk::VMError(_, _) => host_fns::ResultCode::VmError,
                RunOk::FatalVMError(_, _) => host_fns::ResultCode::FatalVmError,
            },
            data: match run_ok {
                RunOk::Return(buf) => buf,
                RunOk::UserError(val) => val,
                RunOk::VMError(msg, _) => calldata::Value::Str(msg.into()).into(),
                RunOk::FatalVMError(msg, _) => calldata::Value::Str(msg.into()).into(),
            },
            backtrace: None,
            wasm_store_hashes: rt::errors::WasmStoreHashes::default(),
            subvm_hashes: empty_subvm_hashes(),
            storage_changes: Vec::new(),
            emissions: Vec::new(),
        }
    }

    /// Only a returning run carries effects; anything else reports none,
    /// whatever it wrote or emitted before failing
    pub fn discard_effects_unless_returned(&mut self) {
        if self.kind == host_fns::ResultCode::Return {
            return;
        }

        self.storage_changes.clear();
        self.emissions.clear();
    }

    pub fn coalesce_fatal_for_top_level(&mut self) {
        if self.kind == host_fns::ResultCode::FatalVmError {
            self.kind = host_fns::ResultCode::VmError;
        }
    }
}

impl RunOk {
    pub fn empty_return() -> Self {
        Self::Return(calldata::Value::Null.into())
    }
}

impl std::fmt::Display for RunOk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Return(r) => {
                let encoded = calldata::encode_obj(r);
                let str = util::str::decode_utf8(encoded.iter().cloned())
                    .map(|r| match r {
                        Ok('\\') => "\\\\".into(),
                        Ok(c) if c.is_control() || c == '\n' || c == '\x07' => {
                            if c as u32 <= 255 {
                                format!("\\x{:02x}", c as u32)
                            } else {
                                format!("\\u{:04x}", c as u32)
                            }
                        }
                        Ok(c) => c.to_string(),
                        Err(util::str::InvalidSequence(seq)) => seq
                            .iter()
                            .map(|c| format!("\\{:02x}", u32::from(*c)))
                            .join(""),
                    })
                    .join("");
                f.write_fmt(format_args!("Return(\"{str}\")"))
            }
            Self::UserError(r) => write!(f, "UserError({:?})", r),
            Self::VMError(r, _) => f.debug_tuple("VMError").field(r).finish(),
            Self::FatalVMError(r, _) => f.debug_tuple("FatalVMError").field(r).finish(),
        }
    }
}
pub struct WasmtimeStoreData {
    pub(super) genlayer_ctx: wasi::Context,
    pub(super) limits: rt::memlimiter::Limiter,
}

impl WasmtimeStoreData {
    pub fn genlayer_ctx_mut(&mut self) -> &mut wasi::Context {
        &mut self.genlayer_ctx
    }
}

pub struct VM<T> {
    pub(super) vm_base: VMBase,
    pub(super) data: T,
}

impl VM<wasmtime::Instance> {
    pub async fn run(mut self) -> Result<RunResult, rt::SpawnError> {
        log_debug!(
            wasi_preview1: serde = self.vm_base.store.data().genlayer_ctx.preview1.log(),
            genlayer_sdk: serde = self.vm_base.store.data().genlayer_ctx.genlayer_sdk.log();
            "run"
        );

        let func = self
            .data
            .get_typed_func::<(), ()>(&mut self.vm_base.store, "")
            .or_else(|_| {
                self.data
                    .get_typed_func::<(), ()>(&mut self.vm_base.store, "_start")
            });

        let func = match func {
            Ok(func) => func,
            Err(e) => {
                return Ok(RunResult {
                    run_ok: RunOk::VMError(
                        public_abi::VmError::invalid_contract().wasm().entrypoint(),
                        Some(crate::wasmtime_to_anyhow(e)),
                    ),
                    backtrace: None,
                    wasm_store_hashes: self.vm_base.wasm_store_hashes(),
                    vm_data: Box::new(
                        self.vm_base
                            .store
                            .into_data()
                            .genlayer_ctx
                            .genlayer_sdk
                            .data,
                    ),
                });
            }
        };

        log_debug!("execution start");
        let time_start = std::time::Instant::now();
        let res = func.call_async(&mut self.vm_base.store, ()).await;
        log_debug!(
            elapsed:? = self.vm_base.store.data().genlayer_ctx.genlayer_sdk.start_time.elapsed(),
            wasm_start_elapsed:? = time_start.elapsed();
            "vm execution finished"
        );
        let res: anyhow::Result<(rt::vm::RunOk, Option<rt::errors::Backtrace>)> = match res {
            Ok(()) => Ok((rt::vm::RunOk::empty_return(), None)),
            Err(e) => {
                let e = rt::errors::UnwrapDynError::from(e);
                if self.vm_base.config_copy.needs_error_fingerprint {
                    // The store is still alive here and holds the wasm memory
                    // state as left by the trapping execution, so take the
                    // memory fingerprint directly from it.
                    rt::errors::unwrap_vm_errors_backtrace(e)
                } else {
                    rt::errors::unwrap_vm_errors(e).map(|run_ok| (run_ok, None))
                }
            }
        };

        let wasm_store_hashes = self.vm_base.wasm_store_hashes();

        match &res {
            Ok((rt::vm::RunOk::Return(_), _)) => {
                log_debug!(result = "Return"; "execution result unwrapped")
            }
            Ok((rt::vm::RunOk::UserError(msg), _)) => {
                log_debug!(result = "UserError", message:cd = msg.clone(); "execution result unwrapped")
            }
            Ok((rt::vm::RunOk::VMError(e, cause), _)) => {
                log_debug!(result = "VMError", message = e.0, cause:? = cause; "execution result unwrapped")
            }
            Ok((rt::vm::RunOk::FatalVMError(e, cause), _)) => {
                log_debug!(result = "FatalVMError", message = e.0, cause:? = cause; "execution result unwrapped")
            }
            Err(e) => {
                log_debug!(result = "Error", error:ah = e; "execution result unwrapped")
            }
        };

        match res {
            Ok((run_ok, backtrace)) => {
                let vm_data = Box::new(
                    self.vm_base
                        .store
                        .into_data()
                        .genlayer_ctx
                        .genlayer_sdk
                        .data,
                );
                Ok(RunResult {
                    run_ok,
                    backtrace,
                    wasm_store_hashes,
                    vm_data,
                })
            }
            Err(e) => Err(rt::SpawnError {
                error: e,
                state: Box::new(rt::SpawnErrorState::Spawned(Box::new(self.vm_base))),
            }),
        }
    }
}

impl<T> VM<T> {
    pub fn map(mut self, f: impl FnOnce(&mut VMBase, T) -> T) -> VM<T> {
        VM {
            data: f(&mut self.vm_base, self.data),
            vm_base: self.vm_base,
        }
    }
}

pub struct VMBase {
    pub(super) store: wasmtime::Store<WasmtimeStoreData>,
    pub(super) linker: wasmtime::Linker<WasmtimeStoreData>,
    pub(super) config_copy: wasi::base::Config,
}

impl VMBase {
    pub fn wasm_store_hashes(&mut self) -> rt::errors::WasmStoreHashes {
        rt::errors::WasmStoreHashes(self.store.fingerprint().module_instances)
    }
}

/// A [`calldata::Writer`] that feeds the encoded bytes straight into a sha3
/// digest, used to hash a value without materializing its encoding.
pub(crate) struct Sha3Writer(pub sha3::Sha3_256);

impl calldata::Writer for &mut Sha3Writer {
    type Error = std::convert::Infallible;

    fn write_all(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        sha3::Digest::update(&mut self.0, data);
        Ok(())
    }
}

impl RunResult {
    /// Fingerprint an in-process caller folds for this child.
    ///
    /// Deliberately the same definition a caller in another executor folds for a
    /// child reached over the nested protocol (see
    /// [`genvm_modules_interfaces::small_hash`]) -- if the two ever drift, the
    /// same call hashes differently depending on how the host routed it.
    pub fn small_hash(&self) -> [u8; 32] {
        use genvm_modules_interfaces::{small_hash, ResultCode};
        use sha3::Digest as _;

        let subvm_hashes = self.vm_data.det_subvm_hashes.clone().finalize();
        let wasm_store_hashes = &self.wasm_store_hashes;

        match &self.run_ok {
            RunOk::Return(buf) => {
                small_hash(ResultCode::Return, buf, &subvm_hashes, wasm_store_hashes)
            }
            RunOk::UserError(buf) => {
                small_hash(ResultCode::UserError, buf, &subvm_hashes, wasm_store_hashes)
            }
            RunOk::VMError(data, _) => small_hash(
                ResultCode::VmError,
                &calldata::Value::Str(data.0.to_string()),
                &subvm_hashes,
                wasm_store_hashes,
            ),
            RunOk::FatalVMError(data, _) => small_hash(
                ResultCode::FatalVmError,
                &calldata::Value::Str(data.0.to_string()),
                &subvm_hashes,
                wasm_store_hashes,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fatal_result_is_not_contract_observable_bytes() {
        let code = public_abi::VmError::timeout();
        let err = RunOk::FatalVMError(code.clone(), None)
            .into_contract_observable_bytes()
            .unwrap_err();

        let recovered = err.into_run_ok().expect("fatal VM error must stay typed");
        match recovered {
            RunOk::FatalVMError(recovered, _) => assert_eq!(recovered, code),
            other => panic!("expected FatalVMError, got {other:?}"),
        }
    }

    #[test]
    fn fatal_result_is_reported_as_fatal() {
        let full =
            FullResult::empty_from(RunOk::FatalVMError(public_abi::VmError::timeout(), None));

        assert_eq!(full.kind, host_fns::ResultCode::FatalVmError);
    }

    #[test]
    fn top_level_fatal_result_is_coalesced() {
        let mut full =
            FullResult::empty_from(RunOk::FatalVMError(public_abi::VmError::timeout(), None));

        full.coalesce_fatal_for_top_level();

        assert_eq!(full.kind, host_fns::ResultCode::VmError);
    }

    fn with_effects(run_ok: RunOk) -> FullResult {
        let mut full = FullResult::empty_from(run_ok);
        full.storage_changes = vec![storage::Delta::for_test([7; 36], vec![1, 2, 3])];
        full.emissions = vec![domain::ExecutionEmission::EthSend {
            address: calldata::Address::zero(),
            calldata: bytes::Bytes::from_static(b"payload"),
            value: primitive_types::U256::zero(),
            message_fee: primitive_types::U256::zero(),
            receipt_fee: primitive_types::U256::zero(),
            fee_params: abi::fees::ExternalMessageParams {
                gas_limit: primitive_types::U256::zero(),
                max_gas_price: primitive_types::U256::zero(),
            },
        }];
        full
    }

    #[test]
    fn returning_run_keeps_its_effects() {
        let mut full = with_effects(RunOk::empty_return());
        full.discard_effects_unless_returned();

        assert_eq!(full.storage_changes.len(), 1);
        assert_eq!(full.emissions.len(), 1);
    }

    #[test]
    fn non_returning_run_reports_no_effects() {
        for run_ok in [
            RunOk::UserError(calldata::Value::Null.into()),
            RunOk::VMError(public_abi::VmError::timeout(), None),
            RunOk::FatalVMError(public_abi::VmError::timeout(), None),
        ] {
            let mut full = with_effects(run_ok);
            full.discard_effects_unless_returned();

            assert!(full.storage_changes.is_empty(), "{:?}", full.kind);
            assert!(full.emissions.is_empty(), "{:?}", full.kind);
        }
    }
}
