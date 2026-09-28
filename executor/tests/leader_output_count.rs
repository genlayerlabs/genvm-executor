use genvm::{
    public_abi::VmError,
    rt::{vm::RunOk, RunMode},
    validate_leader_output_count,
};

#[test]
fn startup_fee_errors_with_unused_outputs_are_leader_faults() {
    for mode in [RunMode::Validator, RunMode::Sync] {
        for error in [
            VmError::out_of().storage(),
            VmError::out_of().receipt().message().val(),
            VmError::out_of().receipt().nondet_output(),
            VmError::out_of().message_fee().total().val(),
            VmError::out_of().receipt().event(),
        ] {
            let expected = genvm::rt::errors::vm_error_for_leader_extra(
                genvm::rt::vm::ContractOutcome::VMError(error.clone(), None)
                    .encode()
                    .as_slice(),
            );
            let outcome = RunOk::VMError(error, None);
            assert_eq!(
                validate_leader_output_count(mode, 0, 1, &outcome),
                Some(expected)
            );
            assert_eq!(validate_leader_output_count(mode, 0, 0, &outcome), None);
        }
    }
}

#[test]
fn only_surplus_outputs_in_consumer_modes_are_rejected() {
    let outcome = RunOk::VMError(VmError::out_of().memory().val(), None);
    for mode in [RunMode::Leader, RunMode::Validator, RunMode::Sync] {
        for (executed, published) in [(0, 0), (1, 0), (1, 1), (1, 2)] {
            let error = validate_leader_output_count(mode, executed, published, &outcome);
            assert_eq!(
                error.is_some(),
                mode != RunMode::Leader && published > executed as usize
            );
        }
    }
}

#[test]
fn existing_fatal_errors_take_precedence_over_surplus_outputs() {
    for mode in [RunMode::Validator, RunMode::Sync] {
        let outcome =
            RunOk::FatalVMError(VmError::leader_fault().nondet_output().malformed(), None);
        assert_eq!(validate_leader_output_count(mode, 0, 1, &outcome), None);
    }
}
