use super::*;

#[tokio::test]
async fn exhausted_message_buckets_report_resource_not_fee() {
    let mut failures = Vec::new();
    for (bucket, expected) in [
        ("submitted_messages", "out_of message_bytes"),
        ("submitted_messages_count", "out_of message_count"),
    ] {
        for (emission, unallocated) in [
            (MessageEmission::InternalAllocation, false),
            (MessageEmission::InternalBalance, false),
            (MessageEmission::DeployAllocation, false),
            (MessageEmission::DeployBalance, false),
            (MessageEmission::External, false),
            (MessageEmission::External, true),
        ] {
            let mut fees = emission_fees();
            fees.message_receipt.buckets.push(bucket.into());
            fees.message_receipt.delta_expr = "\\attrs = [1, 101]".to_owned();
            let mut test = EmissionTestContext::with_fees(u32::MAX, 100, fees);
            if unallocated {
                test.context.data.accumulator.message_fee_allocation.clear();
                test.context
                    .data
                    .accumulator
                    .message_fee_allocation_consumed
                    .clear();
            }
            let memory_before = test.context.limiter.get_remaining_memory();
            let error = trap_message(test.emit_message(emission).await.unwrap_err());
            if !error.contains(expected) {
                failures.push(format!("{bucket} {}: {error}", emission.name()));
            }
            assert!(test.context.data.accumulator.emissions.is_empty());
            assert_eq!(test.context.limiter.get_remaining_memory(), memory_before);
            let consumed = test
                .context
                .data
                .supervisor
                .shared_data
                .data_fees_limit
                .consumed()
                .await;
            assert_eq!(consumed.message_fee, U256::zero());
            assert_eq!(consumed.message_receipt, U256::zero());
            test.shutdown().await;
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
