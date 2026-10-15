use super::*;
use genlayer_sdk::abi;

fn internal(message_fee: u64, use_balance: bool) -> ExecutionEmission {
    ExecutionEmission::InternalMessage {
        call_key: abi::CallKey([0; 32]),
        address: genlayer_sdk::calldata::Address::zero(),
        calldata: abi::entry::MainCallData {
            name: None,
            args: None,
            kwargs: None,
        },
        value: U256::zero(),
        on: abi::gl_call::On::Finalized,
        message_fee: U256::from(message_fee),
        receipt_fee: U256::from(7),
        fee_params: abi::fees::InternalMessageParams {
            leader_time_units_allocation: U256::one(),
            validator_time_units_allocation: U256::one(),
            execution_budget_per_round: U256::one(),
            rotations: vec![U256::one()],
            max_price_gen_per_time_unit: U256::one(),
            storage_fee_max_gas_price: U256::one(),
            receipt_fee_max_gas_price: U256::one(),
        },
        subtree: bytes::Bytes::new(),
        use_balance,
    }
}

fn external(message_fee: u64) -> ExecutionEmission {
    ExecutionEmission::ExternalMessage {
        address: genlayer_sdk::calldata::Address::zero(),
        calldata: bytes::Bytes::new(),
        value: U256::zero(),
        message_fee: U256::from(message_fee),
        receipt_fee: U256::zero(),
        fee_params: abi::fees::ExternalMessageParams {
            gas_limit: U256::one(),
            max_gas_price: U256::one(),
        },
    }
}

fn cascade(
    min_primary_fee: u64,
    budget_remaining: u64,
) -> genvm_modules_interfaces::fees::MessageFeeCascade {
    genvm_modules_interfaces::fees::MessageFeeCascade {
        min_primary_fee: U256::from(min_primary_fee),
        budget_remaining: U256::from(budget_remaining),
        external_fee_reserve: U256::zero(),
    }
}

fn fees(emissions: &[ExecutionEmission]) -> Vec<u64> {
    emissions
        .iter()
        .map(|e| match e {
            ExecutionEmission::InternalMessage { message_fee, .. }
            | ExecutionEmission::InternalDeployMessage { message_fee, .. }
            | ExecutionEmission::ExternalMessage { message_fee, .. } => message_fee.low_u64(),
            ExecutionEmission::Event { .. } => 0,
        })
        .collect()
}

#[test]
fn delivered_child_reduces_the_next_generation_split() {
    let mut emissions = vec![internal(0, false)];
    apply(&mut emissions, &cascade(2, 400));
    assert_eq!(fees(&emissions), vec![201]);
    let mut replacement = vec![internal(0, false)];
    apply(&mut replacement, &cascade(2, 199));
    assert_eq!(fees(&replacement), vec![100]);
}

#[test]
fn reserves_each_external_envelope_before_splitting() {
    let mut emissions = vec![internal(10, false), external(1), external(1)];
    let mut params = cascade(30, 1_000);
    params.external_fee_reserve = U256::from(400);
    apply(&mut emissions, &params);
    assert_eq!(fees(&emissions), vec![115, 1, 1]);
}

#[test]
fn insufficient_remaining_budget_still_applies_floors() {
    let mut emissions = vec![internal(10, false), internal(10, false), external(1)];
    let mut params = cascade(30, 20);
    params.external_fee_reserve = U256::MAX;
    apply(&mut emissions, &params);
    assert_eq!(fees(&emissions), vec![30, 30, 1]);
}

#[test]
fn floors_then_splits_half_of_spare() {
    let mut emissions = vec![internal(10, false), internal(40, false), external(5)];
    apply(&mut emissions, &cascade(30, 1_000));
    // floors 30 + 40 = 70, spare 930, per child 930 / 4 = 232
    assert_eq!(fees(&emissions), vec![262, 272, 5]);
}

#[test]
fn balance_funded_messages_are_neither_floored_nor_counted() {
    let mut emissions = vec![internal(10, true), internal(10, false)];
    apply(&mut emissions, &cascade(30, 130));
    // spare 100 over 1 child
    assert_eq!(fees(&emissions), vec![10, 80]);
}

#[test]
fn no_spare_only_floors() {
    let mut emissions = vec![internal(10, false), internal(10, false)];
    apply(&mut emissions, &cascade(30, 60));
    assert_eq!(fees(&emissions), vec![30, 30]);
}

#[test]
fn share_rounding_to_zero_only_floors() {
    let mut emissions = vec![internal(30, false), internal(30, false)];
    apply(&mut emissions, &cascade(30, 63));
    assert_eq!(fees(&emissions), vec![30, 30]);
}

#[test]
fn saturated_sum_is_not_spare() {
    let mut emissions = vec![internal(0, false), internal(0, false)];
    let mut big = cascade(0, 0);
    big.min_primary_fee = U256::MAX;
    big.budget_remaining = U256::MAX;
    apply(&mut emissions, &big);
    for e in &emissions {
        let ExecutionEmission::InternalMessage { message_fee, .. } = e else {
            unreachable!()
        };
        assert_eq!(*message_fee, U256::MAX);
    }
}
