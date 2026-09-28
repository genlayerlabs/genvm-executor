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

/// Deliberately omits every constant only the internal branch reads
/// (`overlaySplitBps`, the phase-timeout bounds): the external branch must not
/// force those bindings.
fn gas_data(locked_receipt_gas_price: u64) -> std::collections::BTreeMap<String, String> {
    [
        ("storageUnitPrice", "1".to_owned()),
        (
            "lockedReceiptGasPrice",
            locked_receipt_gas_price.to_string(),
        ),
        ("receiptGasPerByte", "1".to_owned()),
        ("gasPerChangedSlot", "1".to_owned()),
        ("intrinsicGas", "0".to_owned()),
        ("bootloaderOverhead", "0".to_owned()),
        ("fixedProposeReceiptGas", "0".to_owned()),
        ("fixedMessageRevealGas", "0".to_owned()),
        ("receiptWrapperBytes", "1024".to_owned()),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value))
    .collect()
}

fn fee(locked_receipt_gas_price: u64, gas_limit: u64, max_gas_price: u64) -> U256 {
    let fees = DataLimit::new(
        bucket_totals(),
        default_fees(),
        gas_data(locked_receipt_gas_price),
    )
    .unwrap();

    fees.calculate_message_fee_external(&genlayer_sdk::abi::fees::ExternalMessageParams {
        gas_limit: gas_limit.into(),
        max_gas_price: max_gas_price.into(),
    })
    .unwrap()
    .reported_fee()
}

#[test]
fn external_fee_uses_the_locked_price_when_it_is_lower() {
    assert_eq!(fee(3, 1000, 7), U256::from(3000));
}

#[test]
fn external_fee_uses_the_guest_cap_when_it_is_lower() {
    assert_eq!(fee(7, 1000, 3), U256::from(3000));
}

#[test]
fn external_fee_is_price_agnostic_when_both_agree() {
    assert_eq!(fee(5, 1000, 5), U256::from(5000));
}

#[test]
fn external_reservations_reject_a_zero_locked_receipt_gas_price() {
    let fees = DataLimit::new(bucket_totals(), default_fees(), gas_data(0)).unwrap();

    for gas_limit in [0, 1000] {
        let error = fees
            .calculate_message_fee_external(&genlayer_sdk::abi::fees::ExternalMessageParams {
                gas_limit: gas_limit.into(),
                max_gas_price: U256::from(7),
            })
            .unwrap_err();
        assert!(
            error.to_string().contains("fee below_minimum"),
            "unexpected error for gas limit {gas_limit}: {error}"
        );
    }
}

#[test]
fn external_reservations_accept_the_smallest_positive_locked_price() {
    assert_eq!(fee(1, 1000, 7), U256::from(1000));
    assert_eq!(fee(1, 1000, 0), U256::zero());
}
