use super::message::{validate_balance_fee, FEE_PARAM_COUNT_BITS, FEE_PARAM_PRICE_BITS};
use super::run::{
    call_contract_route, derive_call_contract_permissions, leader_outcome_for_publication,
    leader_outcome_for_validation, nested_run_ok, parse_leader_result, strip_vm_error_detail,
    CallContractRoute,
};
use super::*;
use primitive_types::U256;

fn valid_params() -> abi::fees::InternalMessageParams {
    abi::fees::InternalMessageParams {
        leader_timeunits_allocation: U256::from(5),
        validator_timeunits_allocation: U256::from(5),
        execution_budget_per_round: U256::from(1024),
        rotations: vec![U256::from(4); 5],
        max_price_gen_per_time_unit: U256::from(2),
        storage_fee_max_gas_price: U256::from(20),
        receipt_fee_max_gas_price: U256::from(20),
    }
}

fn errno(e: generated::types::Error) -> generated::types::Errno {
    e.downcast().expect("expected a plain errno, got a trap")
}

#[test]
fn balance_no_permission_is_forbidden() {
    let err = validate_balance_fee(false, true, Some(valid_params())).unwrap_err();
    assert_eq!(errno(err), generated::types::Errno::Forbidden);
}

#[test]
fn balance_without_params_is_inval() {
    let err = validate_balance_fee(true, true, None).unwrap_err();
    assert_eq!(errno(err), generated::types::Errno::Inval);
}

#[test]
fn params_without_use_balance_is_inval() {
    let err = validate_balance_fee(true, false, Some(valid_params())).unwrap_err();
    assert_eq!(errno(err), generated::types::Errno::Inval);
}

#[test]
fn empty_rotations_is_inval() {
    let mut p = valid_params();
    p.rotations.clear();
    let err = validate_balance_fee(true, true, Some(p)).unwrap_err();
    assert_eq!(errno(err), generated::types::Errno::Inval);
}

#[test]
fn zero_price_caps_are_inval() {
    for mutate in [
        (|p: &mut abi::fees::InternalMessageParams| p.max_price_gen_per_time_unit = U256::zero())
            as fn(&mut abi::fees::InternalMessageParams),
        |p| p.storage_fee_max_gas_price = U256::zero(),
        |p| p.receipt_fee_max_gas_price = U256::zero(),
    ] {
        let mut p = valid_params();
        mutate(&mut p);
        let err = validate_balance_fee(true, true, Some(p)).unwrap_err();
        assert_eq!(errno(err), generated::types::Errno::Inval);
    }
}

#[test]
fn huge_magnitude_params_are_inval() {
    // Security-review N1 repro: passes the emptiness/zero checks, but the
    // 2^250 magnitudes would push messageFeeFloor past U256 and trip the
    // evaluator's internal `fee cost exceeds U256 range` abort.
    let p = abi::fees::InternalMessageParams {
        leader_timeunits_allocation: U256::one() << 250,
        validator_timeunits_allocation: U256::zero(),
        execution_budget_per_round: U256::zero(),
        rotations: vec![U256::zero()],
        max_price_gen_per_time_unit: U256::one() << 250,
        storage_fee_max_gas_price: U256::from(20),
        receipt_fee_max_gas_price: U256::from(20),
    };
    let err = validate_balance_fee(true, true, Some(p)).unwrap_err();
    assert_eq!(errno(err), generated::types::Errno::Inval);
}

#[test]
fn huge_rotations_entry_is_inval() {
    let mut p = valid_params();
    p.rotations[2] = U256::one() << 250;
    let err = validate_balance_fee(true, true, Some(p)).unwrap_err();
    assert_eq!(errno(err), generated::types::Errno::Inval);
}

#[test]
fn params_at_magnitude_bounds_pass() {
    let mut p = valid_params();
    p.max_price_gen_per_time_unit = (U256::one() << FEE_PARAM_PRICE_BITS) - 1;
    p.storage_fee_max_gas_price = (U256::one() << FEE_PARAM_PRICE_BITS) - 1;
    p.receipt_fee_max_gas_price = (U256::one() << FEE_PARAM_PRICE_BITS) - 1;
    p.execution_budget_per_round = (U256::one() << FEE_PARAM_PRICE_BITS) - 1;
    p.leader_timeunits_allocation = (U256::one() << FEE_PARAM_COUNT_BITS) - 1;
    p.validator_timeunits_allocation = (U256::one() << FEE_PARAM_COUNT_BITS) - 1;
    p.rotations = vec![(U256::one() << FEE_PARAM_COUNT_BITS) - 1; 5];
    let got = validate_balance_fee(true, true, Some(p.clone())).unwrap();
    assert_eq!(got, Some(p));
}

#[test]
fn valid_balance_params_pass_through() {
    let p = valid_params();
    let got = validate_balance_fee(true, true, Some(p.clone())).unwrap();
    assert_eq!(got, Some(p));
}

#[test]
fn no_balance_no_params_is_allocation_path() {
    let got = validate_balance_fee(true, false, None).unwrap();
    assert_eq!(got, None);
}

#[test]
fn non_null_resolve_selects_nested_path_before_local_spawn() {
    let payload = bytes::Bytes::from_static(b"route");
    let ours = genvm_common::version::CURRENT.major as u8;
    assert_eq!(
        call_contract_route(Some(payload.clone()), ours),
        CallContractRoute::Nested(payload)
    );
    assert_eq!(
        call_contract_route(None, ours),
        CallContractRoute::InProcess
    );
}

#[test]
fn a_major_this_line_does_not_serve_goes_to_the_manager() {
    let unservable = genvm_common::version::CURRENT.major as u8 + 1;
    let CallContractRoute::Nested(payload) = call_contract_route(None, unservable) else {
        panic!("a major this line cannot serve must not stay in-process");
    };
    let routing: genvm_modules_interfaces::ExecutorSelector =
        calldata::decode_obj(&payload).unwrap();
    assert_eq!(
        routing,
        genvm_modules_interfaces::ExecutorSelector::MajorOverride {
            major: unservable as u32
        }
    );
}

#[test]
fn call_contract_child_is_deterministic_only() {
    let parent = base::Permissions {
        deterministic: true,
        write_storage: true,
        send_messages: true,
        call_others: true,
        spawn_nondet: true,
        register_runners: true,
        can_use_balance_for_message_fees: true,
    };
    let child = derive_call_contract_permissions(&parent);

    assert!(child.deterministic);
    assert!(child.call_others);
    assert!(child.register_runners);
    assert!(!child.spawn_nondet);
    assert!(!child.write_storage);
    assert!(!child.send_messages);
    assert!(!child.can_use_balance_for_message_fees);
}

#[test]
fn nested_result_must_be_effect_free() {
    let reply = genvm_modules_interfaces::NestedRunReply {
        result: genvm_modules_interfaces::NestedRunResult {
            kind: genvm_modules_interfaces::ResultCode::Return,
            data: calldata::Value::Null.into(),
        },
        small_hash: bytes::Bytes::from(vec![1; 32]),
        effect_free: false,
    };

    assert!(nested_run_ok(reply).is_err());
}

// ---------------------------------------------------------------------------
// Leader-proposed nondet result validation.
//
// Every case below is leader-authored input on the validator path. None of them
// may trap: a trap would turn "the leader sent garbage" into a validator
// internal error (and a timeout vote), which is a leader-controlled way to
// silence validators. They must all resolve to a VMError the caller can turn
// into a disagreement.
// ---------------------------------------------------------------------------

fn leader_bytes(code: public_abi::ResultCode, payload: &[u8]) -> Vec<u8> {
    let mut res = vec![code as u8];
    res.extend_from_slice(payload);
    res
}

fn malformed() -> public_abi::VmError {
    public_abi::VmError::leader_fault()
        .nondet_output()
        .malformed()
}

#[test]
fn leader_empty_result_is_absent() {
    assert_eq!(
        parse_leader_result(&[]).unwrap_err(),
        public_abi::VmError::leader_fault().nondet_output().absent()
    );
}

#[test]
fn leader_unknown_result_code_is_malformed() {
    for code in [7u8, 0x80, 0xff] {
        assert_eq!(parse_leader_result(&[code]).unwrap_err(), malformed());
    }
}

#[test]
fn leader_internal_error_code_is_malformed() {
    // A proposable result admits only 0/1/2; the host-facing codes share the
    // numbering but are never proposable.
    for code in [
        crate::host::host_fns::ResultCode::InternalError,
        crate::host::host_fns::ResultCode::FatalVmError,
    ] {
        assert_eq!(parse_leader_result(&[code as u8]).unwrap_err(), malformed());
    }
}

#[test]
fn fatal_leader_outcome_cannot_be_published() {
    let result = rt::vm::RunOk::FatalVMError(public_abi::VmError::timeout(), None);

    assert!(leader_outcome_for_publication(result).is_err());
}

#[test]
fn malformed_leader_outcome_is_visible_as_vm_error_but_remains_fatal() {
    let (visible, returned) =
        leader_outcome_for_validation(&[crate::host::host_fns::ResultCode::FatalVmError as u8]);

    assert!(matches!(&visible, rt::vm::ContractOutcome::VMError(..)));
    assert!(matches!(returned, rt::vm::RunOk::FatalVMError(..)));
    assert_eq!(
        visible.encode().as_slice()[0],
        public_abi::ResultCode::VmError as u8
    );
}

#[test]
fn leader_return_with_invalid_calldata_is_malformed() {
    // The hole this closes: the executor used to pass the `Return` payload
    // through undecoded, so bytes like these reached the execution hash before
    // the guest SDK ever got a chance to reject them.
    let data = leader_bytes(public_abi::ResultCode::Return, &[0xff, 0xff, 0xff]);
    assert_eq!(parse_leader_result(&data).unwrap_err(), malformed());
}

#[test]
fn leader_return_with_trailing_bytes_is_malformed() {
    let mut payload = calldata::encode(&calldata::Value::Null);
    payload.push(0x00);
    let data = leader_bytes(public_abi::ResultCode::Return, &payload);
    assert_eq!(parse_leader_result(&data).unwrap_err(), malformed());
}

#[test]
fn leader_return_exceeding_depth_limit_is_malformed() {
    // The undecoded `Return` path bypasses the decoder's depth limit entirely.
    let mut v = calldata::Value::Null;
    for _ in 0..200 {
        v = calldata::Value::Array(vec![v]);
    }
    let data = leader_bytes(public_abi::ResultCode::Return, &calldata::encode(&v));
    assert_eq!(parse_leader_result(&data).unwrap_err(), malformed());
}

#[test]
fn leader_valid_return_is_preserved_bytewise() {
    let value = calldata::Value::Str("ok".to_owned());
    let payload = calldata::encode(&value);
    let data = leader_bytes(public_abi::ResultCode::Return, &payload);

    let got = parse_leader_result(&data).unwrap();
    // Validation must not re-encode: the accepted result has to round-trip to the
    // exact bytes the leader proposed, or validators would hash a different
    // value than the one they agreed to.
    assert_eq!(got.encode().into_bytes().as_ref(), data);
}

#[test]
fn leader_user_error_with_invalid_calldata_is_malformed() {
    let data = leader_bytes(public_abi::ResultCode::UserError, &[0xff, 0xff]);
    assert_eq!(parse_leader_result(&data).unwrap_err(), malformed());
}

#[test]
fn leader_valid_user_error_passes() {
    let payload = calldata::encode(&calldata::Value::Str("boom".to_owned()));
    let data = leader_bytes(public_abi::ResultCode::UserError, &payload);
    assert_eq!(
        parse_leader_result(&data)
            .unwrap()
            .encode()
            .into_bytes()
            .as_ref(),
        data
    );
}

#[test]
fn leader_vm_error_not_utf8_is_malformed() {
    let data = leader_bytes(public_abi::ResultCode::VmError, &[0xff, 0xfe]);
    assert_eq!(parse_leader_result(&data).unwrap_err(), malformed());
}

#[test]
fn leader_vm_error_off_trie_code_is_malformed() {
    // A leader may not invent error codes: the public code must come from the
    // `vm_error` trie.
    for code in [
        "i_made_this_up",
        "timeou",
        "timeoutx",
        "out_of",
        "out_of nonsense",
        "exit_code",
        "exit_code notanumber",
        "exit_code 99999999999999999999",
        "wasm_trap not_a_trap",
        "",
    ] {
        let data = leader_bytes(public_abi::ResultCode::VmError, code.as_bytes());
        assert_eq!(
            parse_leader_result(&data).unwrap_err(),
            malformed(),
            "code {code:?} should be rejected"
        );
    }
}

#[test]
fn leader_vm_error_on_trie_code_passes() {
    for code in [
        "timeout",
        "forbidden",
        "out_of storage",
        "out_of memory",
        "out_of memory wasm_memory",
        "invalid_contract",
        "invalid_contract wasm linking",
        "exit_code 1",
        "exit_code -1",
        "wasm_trap",
        "wasm_trap unreachable",
    ] {
        let data = leader_bytes(public_abi::ResultCode::VmError, code.as_bytes());
        let got = parse_leader_result(&data)
            .unwrap_or_else(|e| panic!("code {code:?} should be accepted, got {e:?}"));
        assert_eq!(got.encode().into_bytes().as_ref(), data);
    }
}

#[test]
fn leader_vm_error_with_detail_is_malformed() {
    // The nondet result channel is detail-free: an honest leader strips its own
    // detail before publishing, so a proposal carrying one is malformed -- not
    // something to strip and accept. Accepting it would reopen a free-form byte
    // channel into the execution hash.
    for code in [
        "timeout # took too long",
        "made_up # detail",
        "timeout # ",
        " # ",
    ] {
        let data = leader_bytes(public_abi::ResultCode::VmError, code.as_bytes());
        assert_eq!(
            parse_leader_result(&data).unwrap_err(),
            malformed(),
            "code {code:?} should be rejected for carrying a detail"
        );
    }
}

#[test]
fn leader_vm_error_exit_code_must_be_canonical() {
    // `+7` and `007` parse as 7, so accepting them would let several
    // byte-different codes name the same error and hash differently.
    for code in [
        "exit_code +7",
        "exit_code 007",
        "exit_code -0",
        "exit_code 2147483648",
    ] {
        let data = leader_bytes(public_abi::ResultCode::VmError, code.as_bytes());
        assert_eq!(
            parse_leader_result(&data).unwrap_err(),
            malformed(),
            "code {code:?} should be rejected as non-canonical"
        );
    }

    for code in [
        "exit_code 0",
        "exit_code 7",
        "exit_code -7",
        "exit_code 2147483647",
    ] {
        let data = leader_bytes(public_abi::ResultCode::VmError, code.as_bytes());
        assert!(
            parse_leader_result(&data).is_ok(),
            "code {code:?} should be accepted"
        );
    }
}

#[test]
fn leader_vm_error_detail_is_rejected_before_the_namespace_remap() {
    // The one branch interaction the two checks have: a derived-namespace code
    // carrying a detail is `malformed`, not a remap. Order matters because the
    // remap hashes the whole proposed string, so running it first would fold an
    // attacker-chosen detail into the derived code.
    for code in [
        "leader_fault nondet_output malformed # x",
        "leader_fault nondet_output absent # x",
    ] {
        let data = leader_bytes(public_abi::ResultCode::VmError, code.as_bytes());
        assert_eq!(
            parse_leader_result(&data).unwrap_err(),
            malformed(),
            "code {code:?}: the detail check must win"
        );
    }
}

#[test]
fn leader_empty_payloads_are_malformed() {
    // A bare result code with no payload: `Return`/`UserError` need at least the
    // encoding of some value, so the empty tail fails the decoder.
    for code in [
        public_abi::ResultCode::Return,
        public_abi::ResultCode::UserError,
    ] {
        assert_eq!(
            parse_leader_result(&leader_bytes(code, &[])).unwrap_err(),
            malformed(),
            "{code:?} with an empty payload should be rejected"
        );
    }
}

#[test]
fn leader_user_error_malformed_payloads_match_return() {
    // `UserError` shares `Return`'s branch shape and must reject the same
    // payloads -- trailing bytes and over-deep nesting included.
    let mut trailing = calldata::encode(&calldata::Value::Null);
    trailing.push(0x00);

    let mut deep = calldata::Value::Null;
    for _ in 0..200 {
        deep = calldata::Value::Array(vec![deep]);
    }

    for payload in [trailing, calldata::encode(&deep)] {
        let data = leader_bytes(public_abi::ResultCode::UserError, &payload);
        assert_eq!(parse_leader_result(&data).unwrap_err(), malformed());
    }
}

#[test]
fn leader_vm_error_in_derived_namespace_is_remapped() {
    // A proposal must never be byte-equal to what a validator derives from
    // rejecting it, or "the leader proposed X" and "the proposal was rejected
    // as X" would be indistinguishable.
    for code in [
        "leader_fault nondet_output absent",
        "leader_fault nondet_output",
        "leader_fault nondet_output malformed",
        "leader_fault nondet_output uses_this_error abcdef",
        "leader_fault nondet_output whatever it wants",
        "leader_fault nondet_output absent extra",
    ] {
        let data = leader_bytes(public_abi::ResultCode::VmError, code.as_bytes());
        let err = parse_leader_result(&data).unwrap_err();

        assert_eq!(
            err,
            rt::errors::vm_error_for_leader_use_this_error(code),
            "code {code:?} should be remapped out of the derived namespace"
        );
        assert_ne!(
            err.0, code,
            "the derived outcome must not be byte-equal to the proposal"
        );
    }
}

#[test]
fn derived_outcomes_are_not_proposable_verbatim() {
    // Feeding a validator-derived outcome back in as a proposal must be
    // rejected again -- the namespace is closed under this loop, which is what
    // the `fix_point` sentinel exists for.
    for seed in ["", "timeout", "leader_fault nondet_output malformed"] {
        let derived = rt::errors::vm_error_for_leader_use_this_error(seed);
        let data = leader_bytes(public_abi::ResultCode::VmError, derived.0.as_bytes());
        assert!(
            parse_leader_result(&data).is_err(),
            "derived outcome {:?} must not be proposable",
            derived.0
        );
    }
}

#[test]
fn leader_vm_error_derived_entry_code_is_malformed() {
    // `malformed_entry` is an executor-derived outcome, so a leader may not
    // claim it as its own result.
    let code = public_abi::VmError::malformed_entry();
    let data = leader_bytes(public_abi::ResultCode::VmError, code.0.as_bytes());
    assert_eq!(parse_leader_result(&data).unwrap_err(), malformed());
}

#[test]
fn every_generated_trie_code_is_accepted() {
    // Guards against codegen drift: adding a `vm_error` trie entry without
    // regenerating `is_valid` would silently make a legitimate leader code
    // unproposable. Mirrors the constructors the generator emits.
    //
    // The leader-fault nondet-output subtree is deliberately absent: it lives
    // in the derived-outcome namespace and so is never proposable (see
    // `leader_vm_error_in_derived_namespace_is_remapped`).
    let codes = [
        public_abi::VmError::timeout(),
        public_abi::VmError::forbidden(),
        public_abi::VmError::wasm_trap().val(),
        public_abi::VmError::wasm_trap().unreachable(),
        public_abi::VmError::out_of().storage(),
        public_abi::VmError::out_of().memory().val(),
        public_abi::VmError::out_of().memory().wasm_memory(),
        public_abi::VmError::out_of().receipt().nondet_output(),
        public_abi::VmError::out_of().message_fee().total(),
        public_abi::VmError::fee().below_minimum(),
        public_abi::VmError::evm().reverted(),
        public_abi::VmError::invalid_contract().val(),
        public_abi::VmError::invalid_contract().wasm().linking(),
        public_abi::VmError::exit_code().val_i32(3),
    ];

    for code in codes {
        assert!(
            public_abi::VmError::is_valid_(&code.0),
            "generated code {:?} must pass its own trie check",
            code.0
        );

        let data = leader_bytes(public_abi::ResultCode::VmError, code.0.as_bytes());
        assert!(
            parse_leader_result(&data).is_ok(),
            "generated code {:?} should be proposable",
            code.0
        );
    }
}

// ---------------------------------------------------------------------------
// Publishing the leader's own result.
// ---------------------------------------------------------------------------

#[test]
fn leader_own_error_loses_only_its_detail() {
    assert_eq!(
        strip_vm_error_detail("timeout # took too long").0,
        "timeout"
    );
    assert_eq!(strip_vm_error_detail("timeout").0, "timeout");
    // Only the first separator splits: a detail may itself contain " # ".
    assert_eq!(
        strip_vm_error_detail("exit_code 1 # a # b").0,
        "exit_code 1"
    );
}

#[test]
fn leader_own_error_is_not_self_filtered() {
    // The acceptance check must not run on the leader's own output: rewriting an
    // honest result into a derived-namespace code would guarantee a hash mismatch
    // against honest validators, which unconditionally replace it again.
    let stripped = strip_vm_error_detail("leader_fault nondet_output absent # inner");
    assert_eq!(stripped.0, "leader_fault nondet_output absent");
    assert!(
        parse_leader_result(&leader_bytes(
            public_abi::ResultCode::VmError,
            stripped.0.as_bytes()
        ))
        .is_err(),
        "sanity: acceptance would have rewritten this code, the publish path must not"
    );
}

#[test]
fn every_stripped_leader_error_round_trips_through_acceptance() {
    // The invariant the publish path relies on: an honest leader's published
    // bytes are exactly what an honest validator accepts. Only the derived
    // namespace is exempt, and the leader cannot reach it (the nondet child
    // cannot spawn a nondet child of its own).
    for code in [
        "timeout",
        "forbidden",
        "wasm_trap unreachable",
        "out_of memory wasm_memory",
        "exit_code 0",
        "exit_code -7",
        "invalid_contract wasm linking",
    ] {
        let published = strip_vm_error_detail(&format!("{code} # some detail"));
        let data = leader_bytes(public_abi::ResultCode::VmError, published.0.as_bytes());

        let accepted = parse_leader_result(&data)
            .unwrap_or_else(|e| panic!("honest leader code {code:?} rejected as {e:?}"));
        assert_eq!(accepted.encode().into_bytes().as_ref(), data);
    }
}
