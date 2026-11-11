use super::super::*;
use crate::wasi::genlayer_sdk::tests::EmissionTestContext;

async fn fixture(det_compiler: bool, nondet_compiler: bool) -> EmissionTestContext {
    let mut fixture = EmissionTestContext::new(u32::MAX, 100);
    await_nondet_vms(&fixture.context.data.supervisor)
        .await
        .unwrap();
    let engine = |compiler| {
        let mut config = wasmtime::Config::new();
        config.enable_compiler(compiler);
        wasmtime::Engine::new(&config).unwrap()
    };
    let supervisor = Arc::get_mut(&mut fixture.context.data.supervisor)
        .expect("stopped supervisor has no worker references");
    supervisor.engines = DetNondet {
        det: engine(det_compiler),
        non_det: engine(nondet_compiler),
    };
    fixture
}

const EMPTY_WASM: &[u8] = b"\0asm\x01\0\0\0";

async fn assert_compiler_failure_is_internal(det_compiler: bool, nondet_compiler: bool) {
    let fixture = fixture(det_compiler, nondet_compiler).await;
    let supervisor = &fixture.context.data.supervisor;
    supervisor.validate_wasm(EMPTY_WASM).unwrap();
    let error = supervisor
        .compile_wasm(EMPTY_WASM, "test.wasm")
        .await
        .err()
        .unwrap();
    assert!(format!("{error:#}").contains("compiler"), "{error:#}");
    assert!(rt::errors::unwrap_vm_errors(error.into()).is_err());
}

#[tokio::test]
async fn deterministic_compiler_failure_is_internal() {
    assert_compiler_failure_is_internal(false, true).await;
}

#[tokio::test]
async fn nondeterministic_compiler_failure_is_internal() {
    assert_compiler_failure_is_internal(true, false).await;
}

#[tokio::test]
async fn invalid_wasm_remains_a_validation_vm_error() {
    let fixture = fixture(true, true).await;
    let supervisor = &fixture.context.data.supervisor;
    let error = supervisor
        .compile_wasm(b"invalid", "test.wasm")
        .await
        .err()
        .unwrap();
    assert!(
        matches!(rt::errors::unwrap_vm_errors(error.into()).unwrap(),
        rt::vm::RunOk::VMError(code, _) if code == public_abi::VmError::invalid_contract().wasm().validating())
    );
    assert!(supervisor
        .compile_wasm(EMPTY_WASM, "test.wasm")
        .await
        .is_ok());
}
