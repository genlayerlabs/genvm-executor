use super::*;
use crate::domain::fees;

fn params() -> fees::InternalMessageParams {
    fees::InternalMessageParams {
        leader_timeunits_allocation: U256::one(),
        validator_timeunits_allocation: U256::one(),
        execution_budget_per_round: U256::one(),
        rotations: vec![U256::one()],
        max_price_gen_per_time_unit: U256::one(),
        storage_fee_max_gas_price: U256::one(),
        receipt_fee_max_gas_price: U256::one(),
    }
}

fn post(message_fee: u64) -> ExecutionEmission {
    ExecutionEmission::PostMessage {
        call_key: genlayer_sdk::abi::CallKey([0; 32]),
        address: genlayer_sdk::calldata::Address::zero(),
        calldata: genlayer_calldata::codec::Maybe::Materialized(
            genlayer_sdk::calldata::Value::Null,
        ),
        value: U256::zero(),
        on: genlayer_sdk::abi::gl_call::On::Finalized,
        message_fee: U256::from(message_fee),
        receipt_fee: U256::zero(),
        fee_params: params(),
        subtree: bytes::Bytes::new(),
    }
}

fn deploy(message_fee: u64) -> ExecutionEmission {
    ExecutionEmission::DeployContract {
        calldata: genlayer_calldata::codec::Maybe::Materialized(
            genlayer_sdk::calldata::Value::Null,
        ),
        code: bytes::Bytes::new(),
        value: U256::zero(),
        on: genlayer_sdk::abi::gl_call::On::Finalized,
        salt_nonce: U256::zero(),
        message_fee: U256::from(message_fee),
        receipt_fee: U256::zero(),
        fee_params: params(),
        subtree: bytes::Bytes::new(),
    }
}

fn fees_of(emissions: &[ExecutionEmission]) -> Vec<u64> {
    emissions
        .iter()
        .map(|e| match e {
            ExecutionEmission::PostMessage { message_fee, .. }
            | ExecutionEmission::DeployContract { message_fee, .. }
            | ExecutionEmission::EthSend { message_fee, .. } => message_fee.low_u64(),
            ExecutionEmission::EmitEvent { .. } => 0,
        })
        .collect()
}

#[test]
fn floors_then_splits_half_of_spare_across_posts_and_deploys() {
    let mut emissions = vec![post(10), deploy(40)];
    apply(
        &mut emissions,
        &genvm_modules_interfaces::fees::MessageFeeCascade {
            min_primary_fee: U256::from(30),
            budget_total: U256::from(1_000),
        },
    );
    // floors 30 + 40 = 70, spare 930, per child 930 / 4 = 232
    assert_eq!(fees_of(&emissions), vec![262, 272]);
}

#[test]
fn no_spare_only_floors() {
    let mut emissions = vec![post(10), post(10)];
    apply(
        &mut emissions,
        &genvm_modules_interfaces::fees::MessageFeeCascade {
            min_primary_fee: U256::from(30),
            budget_total: U256::from(60),
        },
    );
    assert_eq!(fees_of(&emissions), vec![30, 30]);
}
