use genvm::config::FeesConfig;
use genvm::rt::fees::{DataLimit, MessageReceiptParams};
use primitive_types::U256;

fn default_fees() -> FeesConfig {
    let config: genvm::config::Config =
        serde_yaml::from_str(include_str!("../install/config/genvm.yaml")).unwrap();
    config.fees
}

fn gas_data() -> std::collections::BTreeMap<String, String> {
    [
        ("bootloaderOverhead", 60_000),
        ("fixedMessageRevealGas", 100_000),
        ("fixedProposeReceiptGas", 210_000),
        ("gasPerChangedSlot", 1_000),
        ("intrinsicGas", 21_000),
        ("receiptGasPerByte", 16),
        ("receiptWrapperBytes", 1_024),
    ]
    .map(|(name, value)| (name.to_owned(), value.to_string()))
    .into()
}

fn data_limit(
    execution_data_gas: u64,
    submitted_messages: u64,
    submitted_messages_count: u64,
) -> DataLimit {
    DataLimit::new(
        std::collections::HashMap::from([
            (
                "execution_data_gas".to_owned(),
                U256::from(execution_data_gas),
            ),
            ("message_fee".to_owned(), U256::zero()),
            ("nondet_outputs".to_owned(), U256::from(64)),
            (
                "submitted_messages".to_owned(),
                U256::from(submitted_messages),
            ),
            (
                "submitted_messages_count".to_owned(),
                U256::from(submitted_messages_count),
            ),
        ]),
        default_fees(),
        gas_data(),
    )
    .unwrap()
}

fn empty_external_message(is_first_message: bool) -> MessageReceiptParams {
    MessageReceiptParams {
        is_first_message,
        is_internal: false,
        is_deploy: false,
        rotations_count: 0,
        calldata_length: 0,
        code_length: 0,
        subtree_length: 0,
    }
}

#[tokio::test]
async fn message_free_initial_charge_excludes_reveal_cost() {
    let fees = data_limit(315_408, 0, 0);

    assert!(fees.consume_initial().await.is_none());
    assert_eq!(fees.remaining().await["execution_data_gas"], U256::zero());
}

#[tokio::test]
async fn reveal_cost_is_charged_with_only_the_first_message() {
    let fees = data_limit(518_888, 1_280, 2);
    assert!(fees.consume_initial().await.is_none());
    let message = fees
        .calculate_message_receipt(empty_external_message(true))
        .unwrap();
    let next_message = fees
        .calculate_message_receipt(empty_external_message(false))
        .unwrap();

    assert!(fees.consume_message_receipt_only(&message).await);
    assert!(fees.consume_message_receipt_only(&next_message).await);

    let remaining = fees.remaining().await;
    assert_eq!(remaining["execution_data_gas"], U256::zero());
    assert_eq!(remaining["submitted_messages"], U256::zero());
    assert_eq!(remaining["submitted_messages_count"], U256::zero());
}
