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

fn gas_data_without_overlay() -> std::collections::BTreeMap<String, String> {
    [
        ("storageUnitPrice", "1"),
        ("lockedReceiptGasPrice", "1"),
        ("receiptGasPerByte", "1"),
        ("gasPerChangedSlot", "1"),
        ("intrinsicGas", "0"),
        ("bootloaderOverhead", "0"),
        ("fixedProposeReceiptGas", "0"),
        ("fixedMessageRevealGas", "0"),
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
    .collect()
}

fn fee_params() -> genlayer_sdk::abi::fees::InternalMessageParams {
    genlayer_sdk::abi::fees::InternalMessageParams {
        leader_time_units_allocation: U256::one(),
        validator_time_units_allocation: U256::one(),
        execution_budget_per_round: U256::one(),
        rotations: vec![U256::zero()],
        max_price_gen_per_time_unit: U256::one(),
        storage_fee_max_gas_price: U256::one(),
        receipt_fee_max_gas_price: U256::one(),
    }
}

#[test]
fn missing_overlay_split_is_not_treated_as_zero() {
    let fees = DataLimit::new(bucket_totals(), default_fees(), gas_data_without_overlay()).unwrap();

    let error = fees
        .calculate_message_fee_internal(&fee_params())
        .unwrap_err();
    let message = error.to_string();

    assert!(
        message.contains("overlaySplitBps"),
        "unexpected error: {message}"
    );
}
