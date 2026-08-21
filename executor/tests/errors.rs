use genlayer_sdk::abi;
use genvm::rt;
use genvm::rt::errors::{unwrap_vm_errors, Error, ErrorKind, UnwrapDynError};

fn vm_code(run_ok: &rt::vm::RunOk) -> &str {
    match run_ok {
        rt::vm::RunOk::VMError(code, _) => code.0.as_ref(),
        other => panic!("expected VMError, got {other:?}"),
    }
}

fn fatal_vm_code(run_ok: &rt::vm::RunOk) -> &str {
    match run_ok {
        rt::vm::RunOk::FatalVMError(code, _) => code.0.as_ref(),
        other => panic!("expected FatalVMError, got {other:?}"),
    }
}

// Regression: a VM error raised in an `errors::Result` function reaches the
// unwrap path boxed as a concrete `Error` after crossing an `anyhow::Result`
// boundary (e.g. `get_arch` -> `?`) with extra `.context(..)` layers. It must
// still recover as a `VMError`, not collapse to `internal error`.
#[test]
fn unified_error_survives_anyhow_context_roundtrip() {
    let code = abi::consts::VmError::invalid_contract().runner().absent();

    let err: anyhow::Error = Error::vm(code.clone()).into();
    let err = err.context("parsing chain runner for chain:0x01");
    let err = err.context("getting runner for chain:0x01");

    let recovered = unwrap_vm_errors(UnwrapDynError::from(err)).expect("must be a VM result");
    assert_eq!(vm_code(&recovered), code.0.as_ref());
}

// A foreign error type (wasmtime's, in practice) may carry a boxed `Error`
// as its *source* rather than as an anyhow context frame -- e.g. a resource
// limiter denial raised while instantiating a module. `anyhow`'s own
// downcast does not see through that, so the code must still be recovered
// from the chain instead of collapsing to `internal error`.
#[test]
fn unified_error_survives_a_foreign_error_source() {
    let code = abi::consts::VmError::out_of().memory().wasm_memory();

    let err = anyhow::Error::new(Error::vm(code.clone())).context("instantiating \"cpython.wasm\"");
    let err: anyhow::Error = anyhow::Error::new(std::io::Error::other(Box::<
        dyn std::error::Error + Send + Sync,
    >::from(err)));

    let recovered = Error::from(err).into_run_ok().expect("must be a VM result");
    assert_eq!(vm_code(&recovered), code.0.as_ref());
}

#[test]
fn a_foreign_error_without_a_vm_cause_stays_internal() {
    let err = anyhow::Error::new(std::io::Error::other("disk on fire"));

    assert!(
        matches!(Error::from(err).kind, ErrorKind::Internal),
        "a foreign error with no VM cause must not gain a VM code"
    );
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

#[test]
fn wrap_keeps_inner_fatal_vm_code() {
    let inner = abi::consts::VmError::timeout();
    let outer = abi::consts::VmError::invalid_contract().val();

    let wrapped = Error::wrap(outer, Error::fatal_vm(inner.clone()));
    let recovered = wrapped.into_run_ok().expect("must be a VM result");
    assert_eq!(fatal_vm_code(&recovered), inner.0.as_ref());
}

// `Error::wrap` promotes an internal cause to the given VM code while keeping
// the diagnostic message.
#[test]
fn wrap_promotes_internal_cause() {
    let code = abi::consts::VmError::invalid_contract().runner().absent();

    let wrapped = Error::wrap(code.clone(), anyhow::anyhow!("boom"));
    assert!(wrapped.is_vm());
    let recovered = wrapped.into_run_ok().expect("must be a VM result");
    assert_eq!(vm_code(&recovered), code.0.as_ref());
}

// A detail is part of the code by the time an error carries it, so it
// reaches the terminal result verbatim.
#[test]
fn a_detail_rides_along_with_its_code() {
    let code = abi::consts::VmError::out_of()
        .message_fee()
        .allocation_budget()
        .internal();

    let recovered = Error::vm(code).into_run_ok().expect("must be a VM result");
    assert_eq!(
        vm_code(&recovered),
        "out_of message_fee allocation_budget # internal"
    );
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

// A code boxed in a `wasmtime::Error` (the WASI surface reports that way) must
// survive a round trip through `anyhow`, which wasmtime's own conversion loses.
#[test]
fn unified_error_survives_a_wasmtime_layer() {
    let code = abi::consts::VmError::invalid_contract()
        .runner()
        .malformed();

    let err: wasmtime::Error = Error::vm(code.clone()).into();
    let err = genvm::wasmtime_to_anyhow(err);
    let err = genvm::anyhow_to_wasmtime(err);

    let recovered = unwrap_vm_errors(UnwrapDynError::from(err)).expect("must be a VM result");
    assert_eq!(vm_code(&recovered), code.0.as_ref());
}
