use genvm::config::{FeesBucketConfig, FeesConfig};
use genvm::rt::fees::DataLimit;

fn nondet_fees(total: u64) -> DataLimit {
    let bucket = |delta: &str| FeesBucketConfig {
        bucket_no: vec![0],
        subtract_on_start_expr: "0".to_owned(),
        delta_expr: delta.to_owned(),
    };
    let fees = FeesConfig {
        expr_prelude: String::new(),
        storage: bucket("\\attrs = 0"),
        message_receipt: bucket("\\attrs = 0"),
        nondet_output: bucket("\\attrs = attrs.outputLength"),
        message_fee: bucket("\\attrs = 0"),
        event: bucket("\\attrs = 0"),
    };
    DataLimit::new(
        vec![primitive_types::U256::from(total)],
        fees,
        Default::default(),
    )
    .unwrap()
}

#[tokio::test]
async fn nondet_fee_preflight_checks_without_consuming() {
    let fees = nondet_fees(5);

    assert!(fees.can_consume_nondet_output(5).await.unwrap());
    assert!(!fees.can_consume_nondet_output(6).await.unwrap());
    assert_eq!(fees.remaining().await, vec![primitive_types::U256::from(5)]);
    assert_eq!(
        fees.consumed().await.nondet_output,
        primitive_types::U256::zero()
    );
}

#[tokio::test]
async fn nondet_fee_preflight_leaves_the_checked_charge_available() {
    let fees = nondet_fees(5);

    assert!(fees.can_consume_nondet_output(5).await.unwrap());
    assert!(fees.consume_nondet_output(5).await.unwrap());
    assert_eq!(fees.remaining().await, vec![primitive_types::U256::zero()]);
    assert_eq!(
        fees.consumed().await.nondet_output,
        primitive_types::U256::from(5)
    );
}
