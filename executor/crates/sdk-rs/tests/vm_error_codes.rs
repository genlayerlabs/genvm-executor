//! Every `VmError` a constructor can build must be one `is_valid_` accepts:
//! the constructors are what this executor emits, `is_valid_` is what a
//! validator checks a leader-proposed code against.

use genlayer_sdk::abi::consts::VmError;

fn assert_valid(err: VmError) {
    let code: String = err.into();
    assert!(
        VmError::is_valid_(&code),
        "constructed code is rejected: {code:?}"
    );
}

#[test]
fn static_codes_are_valid() {
    assert_valid(VmError::timeout());
    assert_valid(VmError::absent());

    assert_valid(VmError::oom().val());
    assert_valid(VmError::oom().storage());
    assert_valid(VmError::oom().ram().val());
    assert_valid(VmError::oom().ram().table());
    assert_valid(VmError::oom().ram().memory());
    assert_valid(VmError::oom().ram().limit());
    assert_valid(VmError::oom().receipt().nondet_output());
    assert_valid(VmError::oom().receipt().message().internal());
    assert_valid(VmError::oom().receipt().message().external());
    assert_valid(VmError::oom().fees().internal());
    assert_valid(VmError::oom().fees().external());

    assert_valid(VmError::invalid_contract().val());
    assert_valid(VmError::invalid_contract().absent_runner_comment());
    assert_valid(VmError::invalid_contract().not_utf8_text());
    assert_valid(VmError::invalid_contract().malformed_runner());
    assert_valid(VmError::invalid_contract().major_mismatch());
    assert_valid(VmError::invalid_contract().wasm().validating());
    assert_valid(VmError::invalid_contract().wasm().linking());
    assert_valid(VmError::invalid_contract().wasm().entrypoint());
}

#[test]
fn exit_code_is_valid() {
    assert_valid(VmError::exit_code().val_i32(0));
    assert_valid(VmError::exit_code().val_i32(-1));
    assert_valid(VmError::exit_code().val_i32(i32::MAX));
}

#[test]
fn wasm_trap_is_valid() {
    assert_valid(VmError::wasm_trap().val_str("fault"));
    assert_valid(VmError::wasm_trap().val_str("unreachable code executed"));
}

#[test]
fn host_is_valid() {
    assert_valid(VmError::host().val_str("i_o_error"));
}

// ── Empty description: the one input a `$str` constructor must not take ────

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "non-empty description")]
fn wasm_trap_rejects_an_empty_description() {
    let _ = VmError::wasm_trap().val_str("");
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "non-empty description")]
fn host_rejects_an_empty_description() {
    let _ = VmError::host().val_str("");
}
