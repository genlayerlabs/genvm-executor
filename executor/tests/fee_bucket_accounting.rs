use genvm::config::{FeesBucketConfig, FeesConfig};
use genvm::rt::fees::{CostVec, DataLimit};
use primitive_types::U256;

fn config(event: FeesBucketConfig) -> FeesConfig {
    let bucket = || FeesBucketConfig {
        buckets: vec![symbol_table::GlobalSymbol::from("test")],
        subtract_on_start_expr: "0".to_owned(),
        delta_expr: "\\attrs = 0".to_owned(),
    };
    FeesConfig {
        expr_prelude: String::new(),
        storage: bucket(),
        message_receipt: bucket(),
        nondet_output: bucket(),
        message_fee: bucket(),
        event,
    }
}

fn data_limit(fees: FeesConfig) -> DataLimit {
    DataLimit::new(
        std::collections::HashMap::from([("test".to_owned(), U256::MAX)]),
        fees,
        Default::default(),
    )
    .unwrap()
}

#[tokio::test]
async fn duplicate_bucket_cost_overflow_is_rejected_atomically() {
    let event = FeesBucketConfig {
        buckets: vec![
            symbol_table::GlobalSymbol::from("test"),
            symbol_table::GlobalSymbol::from("test"),
        ],
        subtract_on_start_expr: "0".to_owned(),
        delta_expr: format!("\\attrs = [{}, 1]", U256::MAX),
    };
    let fees = data_limit(config(event));

    assert_eq!(fees.consume_event(0, 0).await.unwrap(), None);
    assert_eq!(
        fees.remaining().await,
        std::collections::BTreeMap::from([("test".to_owned(), U256::MAX)])
    );
}

#[tokio::test]
async fn shared_message_bucket_cost_overflow_is_rejected_atomically() {
    let event = FeesBucketConfig {
        buckets: vec![symbol_table::GlobalSymbol::from("test")],
        subtract_on_start_expr: "0".to_owned(),
        delta_expr: "\\attrs = 0".to_owned(),
    };
    let fees = data_limit(config(event));

    assert!(
        !fees
            .consume_message_fee(&CostVec(vec![U256::MAX]), &CostVec(vec![U256::one()]))
            .await
    );
    assert_eq!(
        fees.remaining().await,
        std::collections::BTreeMap::from([("test".to_owned(), U256::MAX)])
    );
}
