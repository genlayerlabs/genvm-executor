use genvm::config::FeesBucketConfig;

fn parse(input: &str) -> Result<FeesBucketConfig, serde_yaml::Error> {
    serde_yaml::from_str(input)
}

#[test]
fn bucket_config_accepts_one_named_bucket() {
    let config = parse("buckets: execution_data_gas\ndelta_expr: '\\a = 0'").unwrap();

    assert_eq!(config.buckets.len(), 1);
    assert_eq!(config.buckets[0].as_str(), "execution_data_gas");
}

#[test]
fn bucket_config_accepts_multiple_named_buckets() {
    let config =
        parse("buckets: [execution_data_gas, submitted_messages]\ndelta_expr: '\\a = 0'").unwrap();

    let names = config
        .buckets
        .iter()
        .map(symbol_table::GlobalSymbol::as_str)
        .collect::<Vec<_>>();
    assert_eq!(names, ["execution_data_gas", "submitted_messages"]);
}

#[test]
fn bucket_config_rejects_numeric_buckets() {
    let error = parse("buckets: 0\ndelta_expr: '\\a = 0'").unwrap_err();
    let message = error.to_string();

    assert!(
        message.contains("non-empty string"),
        "unexpected error: {message}"
    );
}

#[test]
fn bucket_config_rejects_empty_names() {
    let error = parse("buckets: ''\ndelta_expr: '\\a = 0'").unwrap_err();
    let message = error.to_string();

    assert!(
        message.contains("must not be empty"),
        "unexpected error: {message}"
    );
}

#[test]
fn bucket_config_rejects_an_empty_list() {
    let error = parse("buckets: []\ndelta_expr: '\\a = 0'").unwrap_err();
    let message = error.to_string();

    assert!(
        message.contains("at least one entry"),
        "unexpected error: {message}"
    );
}

#[test]
fn bucket_config_rejects_legacy_bucket_number() {
    let error =
        parse("buckets: execution_data_gas\nbucket_no: 0\ndelta_expr: '\\a = 0'").unwrap_err();
    let message = error.to_string();

    assert!(
        message.contains("unknown field `bucket_no`"),
        "unexpected error: {message}"
    );
}
