use genvm::config::FeesConfig;
use genvm::rt::fees::DataLimit;
use primitive_types::U256;

fn default_fees() -> FeesConfig {
    let config: serde_yaml::Value =
        serde_yaml::from_str(include_str!("../install/config/genvm.yaml")).unwrap();
    serde_yaml::from_value(config["fees"].clone()).unwrap()
}

fn bucket_totals() -> std::collections::HashMap<String, U256> {
    [
        "execution_data_gas",
        "message_fee",
        "nondet_outputs",
        "submitted_messages",
        "submitted_messages_count",
    ]
    .into_iter()
    .map(|name| (name.to_owned(), U256::MAX))
    .collect()
}

fn gas_data(
    min_propose: u64,
    max_propose: u64,
    min_commit: u64,
    max_commit: u64,
) -> std::collections::BTreeMap<String, String> {
    [
        ("storageUnitPrice", "1".to_owned()),
        ("lockedReceiptGasPrice", "1".to_owned()),
        ("receiptGasPerByte", "1".to_owned()),
        ("gasPerChangedSlot", "1".to_owned()),
        ("intrinsicGas", "0".to_owned()),
        ("bootloaderOverhead", "0".to_owned()),
        ("fixedProposeReceiptGas", "0".to_owned()),
        ("fixedMessageRevealGas", "0".to_owned()),
        ("overlaySplitBps", "0".to_owned()),
        ("receiptWrapperBytes", "1024".to_owned()),
        ("minProposeTimeout", min_propose.to_string()),
        ("maxProposeTimeout", max_propose.to_string()),
        ("minCommitTimeout", min_commit.to_string()),
        ("maxCommitTimeout", max_commit.to_string()),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value))
    .collect()
}

fn fee_params(
    leader_time_units: u64,
    validator_time_units: u64,
) -> genlayer_sdk::abi::fees::InternalMessageParams {
    genlayer_sdk::abi::fees::InternalMessageParams {
        leader_time_units_allocation: leader_time_units.into(),
        validator_time_units_allocation: validator_time_units.into(),
        execution_budget_per_round: U256::one(),
        rotations: vec![U256::zero()],
        max_price_gen_per_time_unit: U256::one(),
        storage_fee_max_gas_price: U256::one(),
        receipt_fee_max_gas_price: U256::one(),
    }
}

#[test]
fn both_zero_disables_phase_timeout_validation() {
    let fees = DataLimit::new(bucket_totals(), default_fees(), gas_data(5, 10, 20, 30)).unwrap();

    fees.calculate_message_fee_internal(&fee_params(0, 0))
        .unwrap();
}

#[test]
fn phase_specific_bounds_are_inclusive() {
    let fees = DataLimit::new(bucket_totals(), default_fees(), gas_data(5, 5, 10, 10)).unwrap();

    fees.calculate_message_fee_internal(&fee_params(5, 10))
        .unwrap();
}

#[test]
fn nonzero_phase_timeouts_outside_bounds_are_rejected() {
    let fees = DataLimit::new(bucket_totals(), default_fees(), gas_data(5, 10, 20, 30)).unwrap();

    for (leader, validator) in [(4, 20), (11, 20), (5, 19), (5, 31), (0, 20), (5, 0)] {
        let error = fees
            .calculate_message_fee_internal(&fee_params(leader, validator))
            .unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("fee below_minimum"),
            "unexpected error for leader={leader}, validator={validator}: {message}"
        );
    }
}

#[test]
fn primary_fee_is_not_multiplied_by_appeal_lifecycle() {
    let mut gas_data = gas_data(1, u64::MAX, 1, u64::MAX);
    gas_data.insert("overlaySplitBps".to_owned(), "1500".to_owned());
    let fees = DataLimit::new(bucket_totals(), default_fees(), gas_data).unwrap();
    let params = genlayer_sdk::abi::fees::InternalMessageParams {
        leader_time_units_allocation: U256::from(5),
        validator_time_units_allocation: U256::from(5),
        execution_budget_per_round: U256::from(1024),
        rotations: vec![U256::from(4); 5],
        max_price_gen_per_time_unit: U256::from(3),
        storage_fee_max_gas_price: U256::from(20),
        receipt_fee_max_gas_price: U256::from(20),
    };

    let fee = fees.calculate_message_fee_internal(&params).unwrap();

    assert_eq!(fee.reported_fee(), U256::from(47_837));
}
