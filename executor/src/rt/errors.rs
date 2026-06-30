use std::collections::BTreeMap;

use crate::rt;
use genlayer_sdk::abi;
use genvm_common::*;

/// Recover a terminal VM/User result from a boxed executor [`Error`]. An
/// `Internal` error has no VM-level representation and is re-raised unchanged.
///
/// Recognizing [`Error`] here is what keeps a VM error code intact across an
/// `errors::Result` -> `anyhow::Result` boundary: the error reaches this point
/// boxed as a concrete `Error`, so without this arm it would fall through to the
/// generic case and be reported as `internal error`.
fn recover_executor_error(e: UnwrapDynError) -> std::result::Result<rt::vm::RunOk, UnwrapDynError> {
    match e.downcast::<Error>() {
        Ok(err) => err
            .into_run_ok()
            .map_err(|internal| UnwrapDynError::Anyhow(anyhow::Error::new(internal))),
        Err(e) => Err(e),
    }
}

#[allow(clippy::manual_try_fold)]
pub fn unwrap_vm_errors(err: UnwrapDynError) -> anyhow::Result<rt::vm::RunOk> {
    let res: std::result::Result<rt::vm::RunOk, UnwrapDynError> = [
        |e: UnwrapDynError| {
            e.downcast::<wasmtime::Trap>().map(|v| {
                rt::vm::RunOk::VMError(
                    abi::consts::VmError::wasm_trap().val_str(&format!("{v:?}")),
                    Some(v.into()),
                )
            })
        },
        |e: UnwrapDynError| {
            e.downcast::<wiggle::GuestError>().map(|e| {
                rt::vm::RunOk::VMError(
                    abi::consts::VmError::wasm_trap().val_str("fault"),
                    Some(e.into()),
                )
            })
        },
        |e: UnwrapDynError| recover_executor_error(e),
        |e: UnwrapDynError| {
            e.downcast::<crate::wasi::genlayer_sdk::ContractReturn>()
                .map(|crate::wasi::genlayer_sdk::ContractReturn(v)| rt::vm::RunOk::Return(v))
        },
    ]
    .into_iter()
    .fold(Err(err), |acc, func| match acc {
        Ok(acc) => Ok(acc),
        Err(e) => func(e),
    });

    match res {
        Ok(r) => Ok(r),
        Err(UnwrapDynError::Anyhow(e)) => Err(e),
        Err(UnwrapDynError::Wasmtime(e)) => Err(crate::wasmtime_to_anyhow(e)),
    }
}

pub enum UnwrapDynError {
    Anyhow(anyhow::Error),
    Wasmtime(wasmtime::Error),
}

impl From<anyhow::Error> for UnwrapDynError {
    fn from(err: anyhow::Error) -> Self {
        match err.downcast::<wasmtime::Error>() {
            Ok(casted) => From::<wasmtime::Error>::from(casted),
            Err(e) => UnwrapDynError::Anyhow(e),
        }
    }
}

impl From<wasmtime::Error> for UnwrapDynError {
    fn from(err: wasmtime::Error) -> Self {
        match err.downcast::<anyhow::Error>() {
            Ok(casted) => From::<anyhow::Error>::from(casted),
            Err(e) => UnwrapDynError::Wasmtime(e),
        }
    }
}

impl UnwrapDynError {
    fn downcast_ref<E>(&self) -> Option<&E>
    where
        E: std::fmt::Display + std::fmt::Debug + Send + Sync + 'static,
    {
        match self {
            UnwrapDynError::Anyhow(e) => e.downcast_ref::<E>(),
            UnwrapDynError::Wasmtime(e) => e.downcast_ref::<E>(),
        }
    }

    fn downcast<E>(self) -> std::result::Result<E, Self>
    where
        E: std::fmt::Display + std::fmt::Debug + Send + Sync + 'static,
    {
        match self {
            UnwrapDynError::Anyhow(e) => e.downcast::<E>().map_err(UnwrapDynError::Anyhow),
            UnwrapDynError::Wasmtime(e) => e.downcast::<E>().map_err(UnwrapDynError::Wasmtime),
        }
    }
}

/// Recover the call stack carried by a trapping error, if any. This is
/// independent of the wasm store and can be done even after it is consumed.
pub fn extract_backtrace(err: &UnwrapDynError) -> Option<Backtrace> {
    let Some(bt) = err.downcast_ref::<wasmtime::WasmBacktrace>() else {
        log_warn!("no backtrace attached");
        return None;
    };

    let frames = bt
        .frames()
        .iter()
        .map(|f| Frame {
            module_name: f.module().name().unwrap_or("").to_string(),
            func: f.func_index(),
        })
        .collect();

    Some(Backtrace { frames })
}

pub fn unwrap_vm_errors_backtrace(
    err: UnwrapDynError,
) -> anyhow::Result<(rt::vm::RunOk, Option<Backtrace>)> {
    let backtrace = extract_backtrace(&err);

    log_debug!(
        bt:serde = backtrace,
        frames = backtrace.as_ref().map_or(0, |b| b.frames.len());
        "captured backtrace"
    );

    Ok((unwrap_vm_errors(err)?, backtrace))
}

// ── Unified executor error ──────────────────────────────────────────
//
// One owned error type for the whole executor, replacing ad-hoc `anyhow`.
// `kind` separates the three cases the executor cares about; the remaining
// fields are common to all of them. Functions return `rt::errors::Result<T>`;
// `anyhow`/`wasmtime` errors appear only at the wasmtime boundary, where
// `From<anyhow::Error>` folds them in and `Error: std::error::Error` lets a
// converted value flow back out into wasmtime's error type.

#[derive(Debug)]
pub enum ErrorKind {
    /// VM-level error carrying a stable code (-> ResultCode::VMError).
    Vm(abi::consts::VmError),
    /// User/contract error carrying a raw UTF-8 string (-> ResultCode::UserError).
    /// v0.2 ABI: user errors are strings, not calldata values.
    User(String),
    /// Generic internal executor error (the bare `anyhow` case).
    Internal,
}

/// The executor's error type. Fields after `kind` are common to every error.
#[derive(Debug)]
pub struct Error {
    pub kind: ErrorKind,
    /// Human-readable context chain, innermost first.
    pub context: Vec<String>,
    /// Underlying cause, if any.
    pub source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
}

pub type Result<T> = std::result::Result<T, Error>;

/// Fold an [`Error`]'s context chain (innermost-first) and source into a single
/// anyhow cause for diagnostics, or `None` when there is nothing to report. The
/// `kind` is surfaced separately (as the result code), so it is intentionally
/// not included here.
fn fold_cause(
    context: Vec<String>,
    source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
) -> Option<anyhow::Error> {
    let mut acc = source.map(|s| anyhow::Error::msg(s.to_string()));
    for c in context {
        acc = Some(match acc {
            Some(e) => e.context(c),
            None => anyhow::Error::msg(c),
        });
    }
    acc
}

impl Error {
    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Internal,
            context: vec![msg.into()],
            source: None,
        }
    }

    pub fn vm(code: abi::consts::VmError) -> Self {
        Self {
            kind: ErrorKind::Vm(code),
            context: Vec::new(),
            source: None,
        }
    }

    /// Wrap a cause as a VM error with `code`, **keeping the inner terminal
    /// error if it already is one**: if `cause` already carries a `Vm`/`User`
    /// kind it is returned unchanged (so the innermost code wins, instead of
    /// being masked by an outer one); otherwise its `Internal` payload is
    /// promoted to `Vm(code)`, preserving the accumulated context and source.
    pub fn wrap(code: abi::consts::VmError, cause: impl Into<Error>) -> Self {
        let mut cause = cause.into();
        if !cause.is_vm() && !cause.is_user() {
            cause.kind = ErrorKind::Vm(code);
        }
        cause
    }

    /// Build a VM error from a code and an optional anyhow cause (`None` ->
    /// bare code). Convenience for the `Option<anyhow::Error>` carried by
    /// [`rt::vm::RunOk::VMError`].
    pub fn vm_cause(code: abi::consts::VmError, cause: Option<anyhow::Error>) -> Self {
        match cause {
            Some(cause) => Self::wrap(code, cause),
            None => Self::vm(code),
        }
    }

    pub fn user(value: String) -> Self {
        Self {
            kind: ErrorKind::User(value),
            context: Vec::new(),
            source: None,
        }
    }

    pub fn with_source(mut self, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// Add a context frame (outermost-last), mirroring `anyhow::Context`.
    pub fn context(mut self, ctx: impl Into<String>) -> Self {
        self.context.push(ctx.into());
        self
    }

    pub fn is_user(&self) -> bool {
        matches!(self.kind, ErrorKind::User(_))
    }

    pub fn is_vm(&self) -> bool {
        matches!(self.kind, ErrorKind::Vm(_))
    }

    /// Convert into the terminal VM result this error represents. `Vm`/`User`
    /// map to their `RunOk` counterparts (the context/source chain is folded
    /// into the VM error's diagnostic cause); `Internal` has no VM-level
    /// representation and is handed back as `Err(self)` to stay an executor
    /// error.
    pub fn into_run_ok(self) -> std::result::Result<rt::vm::RunOk, Self> {
        let Error {
            kind,
            context,
            source,
        } = self;
        match kind {
            ErrorKind::Vm(code) => Ok(rt::vm::RunOk::VMError(code, fold_cause(context, source))),
            ErrorKind::User(v) => Ok(rt::vm::RunOk::UserError(v)),
            ErrorKind::Internal => Err(Error {
                kind: ErrorKind::Internal,
                context,
                source,
            }),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            ErrorKind::Vm(c) => write!(f, "VMError({})", c.0)?,
            ErrorKind::User(v) => write!(f, "UserError({v:?})")?,
            ErrorKind::Internal => f.write_str("internal error")?,
        }
        for c in self.context.iter().rev() {
            write!(f, ": {c}")?;
        }
        if let Some(s) = &self.source {
            write!(f, ": {s}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|s| s as &(dyn std::error::Error + 'static))
    }
}

impl From<anyhow::Error> for Error {
    fn from(err: anyhow::Error) -> Self {
        // Recover the original kind when a boxed executor `Error` is carried in
        // the anyhow chain, so an `errors::Result` -> `anyhow` -> `errors::Result`
        // round trip across the wasmtime boundary stays typed instead of
        // collapsing to `Internal`.
        match err.downcast::<Error>() {
            Ok(e) => e,
            Err(err) => Self {
                kind: ErrorKind::Internal,
                context: vec![format!("{err:#}")],
                source: None,
            },
        }
    }
}

impl From<wasmtime::Error> for Error {
    fn from(err: wasmtime::Error) -> Self {
        // The wasmtime boundary: unwrap to anyhow first (which recovers a boxed
        // executor `Error` if one is carried inside the wasmtime error).
        Self::from(crate::wasmtime_to_anyhow(err))
    }
}

// `From` for common foreign errors `?`-propagated as Internal. A blanket
// `impl<E: std::error::Error> From<E>` is impossible (it collides with the
// reflexive `From<T> for T`), so the set is enumerated as needed.
macro_rules! internal_from {
    ($($t:ty),* $(,)?) => {$(
        impl From<$t> for Error {
            fn from(e: $t) -> Self {
                Self {
                    kind: ErrorKind::Internal,
                    context: Vec::new(),
                    source: Some(Box::new(e)),
                }
            }
        }
    )*};
}

internal_from!(
    std::io::Error,
    serde_json::Error,
    std::str::Utf8Error,
    std::string::FromUtf8Error,
    std::num::TryFromIntError,
    zip::result::ZipError,
    wasmparser::BinaryReaderError,
);

/// Attach context to a `Result<T, E: Into<Error>>`. Named `ctx`/`with_ctx`
/// (not `context`) to avoid colliding with `anyhow::Context` while both are in
/// scope during the migration.
pub trait ResultExt<T> {
    fn ctx(self, ctx: impl Into<String>) -> Result<T>;
    fn with_ctx<F: FnOnce() -> String>(self, f: F) -> Result<T>;
}

impl<T, E: Into<Error>> ResultExt<T> for std::result::Result<T, E> {
    fn ctx(self, ctx: impl Into<String>) -> Result<T> {
        self.map_err(|e| e.into().context(ctx))
    }
    fn with_ctx<F: FnOnce() -> String>(self, f: F) -> Result<T> {
        self.map_err(|e| e.into().context(f()))
    }
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq, genlayer_calldata::Encode)]
pub struct Frame {
    pub module_name: String,
    pub func: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SingleMemoryFP(#[serde(with = "serde_bytes")] pub [u8; 32]);

impl<W: calldata::Writer> calldata::codec::Encode<W> for SingleMemoryFP {
    type Error = W::Error;

    fn encode(&self, enc: &mut calldata::Encoder<W>) -> std::result::Result<(), Self::Error> {
        enc.push_bytes(&self.0)
    }
}

/// The wasm call stack captured at the point of a trap. Carried by the error
/// itself, so it survives even when the store is gone.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct Backtrace {
    pub frames: Vec<Frame>,
}

impl<W: calldata::Writer> calldata::codec::Encode<W> for Backtrace {
    type Error = W::Error;

    fn encode(&self, enc: &mut calldata::Encoder<W>) -> std::result::Result<(), Self::Error> {
        calldata::codec::Encode::encode(&self.frames, enc)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WasmStoreHashes(pub BTreeMap<String, wasmtime::ModuleFingerprint>);

impl<W: calldata::Writer> calldata::codec::Encode<W> for WasmStoreHashes {
    type Error = W::Error;

    fn encode(&self, enc: &mut calldata::Encoder<W>) -> std::result::Result<(), Self::Error> {
        enc.start_map(self.0.len() as u64)?;
        for (k, v) in &self.0 {
            enc.push_map_k(k)?;
            encode_module_fingerprint(v, enc)?;
        }
        Ok(())
    }
}

fn encode_module_fingerprint<W: calldata::Writer>(
    mfp: &wasmtime::ModuleFingerprint,
    enc: &mut calldata::Encoder<W>,
) -> std::result::Result<(), W::Error> {
    // ModuleFingerprint has a single field: memories: Vec<MemoryFingerprint>
    enc.start_map(1)?;
    enc.push_map_k("memories")?;
    enc.start_array(mfp.memories.len() as u64)?;
    for mem in &mfp.memories {
        enc.push_bytes(&mem.0)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vm_code(run_ok: &rt::vm::RunOk) -> &str {
        match run_ok {
            rt::vm::RunOk::VMError(code, _) => code.0.as_ref(),
            other => panic!("expected VMError, got {other:?}"),
        }
    }

    // Regression: a VM error raised in an `errors::Result` function reaches the
    // unwrap path boxed as a concrete `Error` after crossing an `anyhow::Result`
    // boundary (e.g. `get_arch` -> `?`) with extra `.context(..)` layers. It must
    // still recover as a `VMError`, not collapse to `internal error`.
    #[test]
    fn unified_error_survives_anyhow_context_roundtrip() {
        let code = abi::consts::VmError::invalid_contract().absent_runner_comment();

        let err: anyhow::Error = Error::vm(code.clone()).into();
        let err = err.context("parsing chain runner for chain:0x01");
        let err = err.context("getting runner for chain:0x01");

        let recovered = unwrap_vm_errors(UnwrapDynError::from(err)).expect("must be a VM result");
        assert_eq!(vm_code(&recovered), code.0.as_ref());
    }

    // `Error::wrap` keeps the inner terminal code: wrapping an existing VM error
    // with a different code must not mask the original (innermost) one.
    #[test]
    fn wrap_keeps_inner_vm_code() {
        let inner = abi::consts::VmError::invalid_contract().not_utf8_text();
        let outer = abi::consts::VmError::invalid_contract().val();

        let wrapped = Error::wrap(outer, Error::vm(inner.clone()));
        let recovered = wrapped.into_run_ok().expect("must be a VM result");
        assert_eq!(vm_code(&recovered), inner.0.as_ref());
    }

    // `Error::wrap` promotes an internal cause to the given VM code while keeping
    // the diagnostic message.
    #[test]
    fn wrap_promotes_internal_cause() {
        let code = abi::consts::VmError::invalid_contract().absent_runner_comment();

        let wrapped = Error::wrap(code.clone(), anyhow::anyhow!("boom"));
        assert!(wrapped.is_vm());
        let recovered = wrapped.into_run_ok().expect("must be a VM result");
        assert_eq!(vm_code(&recovered), code.0.as_ref());
    }

    // An internal error stays internal (no spurious VM code).
    #[test]
    fn internal_error_stays_internal() {
        let err: anyhow::Error = anyhow::anyhow!("boom");
        let recovered = unwrap_vm_errors(UnwrapDynError::from(err));
        assert!(
            recovered.is_err(),
            "internal error must not become a VM result"
        );
    }
}
