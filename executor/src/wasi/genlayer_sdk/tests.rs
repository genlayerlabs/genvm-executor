use super::message::{validate_balance_fee, FEE_PARAM_COUNT_BITS, FEE_PARAM_PRICE_BITS};
use super::*;
use primitive_types::U256;

fn valid_params() -> domain::fees::InternalMessageParams {
    domain::fees::InternalMessageParams {
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
        (|p: &mut domain::fees::InternalMessageParams| p.max_price_gen_per_time_unit = U256::zero())
            as fn(&mut domain::fees::InternalMessageParams),
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
    let p = domain::fees::InternalMessageParams {
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
