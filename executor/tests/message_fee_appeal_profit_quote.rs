//! Primary-fee quote conformance for executor PR #45 and consensus PR #1692.
//!
//! Node PR #2017 still pins manager 89c20400 and executor e813da59, whose
//! appealed-child quote omits successful-appeal profit. Executor #45 adds that
//! reserve. Keep its quote aligned with the chain before emissions consume a
//! pinned allocation; clamping only afterward can overrun that allocation.

use genvm::config::FeesConfig;
use genvm::rt::fees::DataLimit;
use genvm_modules_interfaces::{
    On,
    fees::{InternalMessageParams, MessageAllocationNode, MessageAllocationNodeParams},
};
use primitive_types::U256;
use std::{collections::HashMap, sync::Arc};

fn fees() -> DataLimit {
    let config: serde_yaml::Value =
        serde_yaml::from_str(include_str!("../install/config/genvm.yaml")).unwrap();
    let fees: FeesConfig = serde_yaml::from_value(config["fees"].clone()).unwrap();
    let buckets: HashMap<_, _> = [
        "execution_data_gas",
        "message_fee",
        "nondet_outputs",
        "submitted_messages",
        "submitted_messages_count",
    ]
    .into_iter()
    .map(|name| (name.to_owned(), U256::MAX))
    .collect();
    let gas_data = [
        ("storageUnitPrice", "1"),
        ("lockedReceiptGasPrice", "1"),
        ("receiptGasPerByte", "1"),
        ("gasPerChangedSlot", "1"),
        ("intrinsicGas", "0"),
        ("bootloaderOverhead", "0"),
        ("fixedProposeReceiptGas", "0"),
        ("fixedMessageRevealGas", "0"),
        ("overlaySplitBps", "1500"),
        ("receiptWrapperBytes", "1024"),
        ("minProposeTimeout", "1"),
        (
            "maxProposeTimeout",
            "340282366920938463463374607431768211455",
        ),
        ("minCommitTimeout", "1"),
        (
            "maxCommitTimeout",
            "340282366920938463463374607431768211455",
        ),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value.to_owned()))
    .collect();
    DataLimit::new(buckets, fees, gas_data).unwrap()
}

fn child_params(
    child_appeal_rounds: usize,
    execution_budget_per_round: u64,
) -> InternalMessageParams {
    InternalMessageParams {
        leader_timeunits_allocation: U256::from(5),
        validator_timeunits_allocation: U256::from(5),
        execution_budget_per_round: U256::from(execution_budget_per_round),
        rotations: vec![U256::from(4); child_appeal_rounds + 1],
        max_price_gen_per_time_unit: U256::from(3),
        storage_fee_max_gas_price: U256::from(20),
        receipt_fee_max_gas_price: U256::from(20),
    }
}

fn emitted_child_quote(
    fees: &DataLimit,
    on: On,
    child_appeal_rounds: usize,
    execution_budget_per_round: u64,
) -> U256 {
    let allocation = MessageAllocationNode {
        recipient: None,
        call_key: None,
        budget: Some(U256::MAX),
        on,
        fee_params: MessageAllocationNodeParams::Internal(Arc::new(child_params(
            child_appeal_rounds,
            execution_budget_per_round,
        ))),
        children_budget: U256::zero(),
        subtree: bytes::Bytes::new(),
    };
    let matched = allocation
        .matches_internal(
            on,
            genlayer_sdk::calldata::Address::from([0; 20]),
            genvm_modules_interfaces::CallKey([0; 32]),
        )
        .unwrap();
    let params = genlayer_sdk::abi::fees::InternalMessageParams {
        leader_time_units_allocation: matched.leader_timeunits_allocation,
        validator_time_units_allocation: matched.validator_timeunits_allocation,
        execution_budget_per_round: matched.execution_budget_per_round,
        rotations: matched.rotations.clone(),
        max_price_gen_per_time_unit: matched.max_price_gen_per_time_unit,
        storage_fee_max_gas_price: matched.storage_fee_max_gas_price,
        receipt_fee_max_gas_price: matched.receipt_fee_max_gas_price,
    };
    fees.calculate_message_fee_internal(&params)
        .unwrap()
        .reported_fee()
}

#[test]
fn zero_execution_budget_no_appeal_quote_matches_chain() {
    // Same parameters except appealRounds=0 and rotations=[4]. The deployed
    // FeeManager.minMessagePrimaryFees also returns 529.
    assert_eq!(
        emitted_child_quote(&fees(), On::Decided, 0, 0),
        U256::from(529)
    );
}

#[test]
fn appealed_child_quote_matches_chain_minimum() {
    // The deployed FeeManager.minMessagePrimaryFees returns 18,238. Executor
    // #45 includes the 9,450 appeal-profit reserve missing from e813da59.
    assert_eq!(
        emitted_child_quote(&fees(), On::Decided, 3, 0),
        U256::from(18_238)
    );
}
