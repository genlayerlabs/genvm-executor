use super::{external_allocation_candidates, resolve_internal_allocation};
use crate::{calldata, domain};
use genlayer_sdk::abi::{self, gl_call};
use primitive_types::U256;
use std::sync::Arc;

#[tokio::test]
async fn unallocated_external_does_not_evaluate_allocation_fee() {
    let bucket = |delta: &str| crate::config::FeesBucketConfig {
        buckets: vec![symbol_table::GlobalSymbol::from("test")],
        subtract_on_start_expr: "0".to_owned(),
        delta_expr: delta.to_owned(),
    };
    let shared = crate::rt::SharedData {
        is_sync: false,
        genvm_id: genvm_modules_interfaces::GenVMId(0),
        debug_mode: genvm_common::DebugMode::Disabled,
        metrics: Default::default(),
        data_fees_limit: crate::rt::fees::DataLimit::new(
            std::collections::HashMap::from([("test".to_owned(), U256::from(10))]),
            crate::config::FeesConfig {
                expr_prelude: String::new(),
                storage: bucket("\\a = 0"),
                message_receipt: bucket("\\a = 1"),
                nondet_output: bucket("\\a = 0"),
                message_fee: bucket("\\a = 1 / a.matchedFeeParams.gasLimit"),
                event: bucket("\\a = 0"),
            },
            Default::default(),
        )
        .unwrap(),
        det_fuel_budget: crate::rt::DetFuelBudget::new(None),
        llm_consumption: tokio::sync::Mutex::new(U256::zero()),
    };
    let mut consumed = U256::zero();
    let fees = super::consume_message_fee_external(
        &shared,
        None,
        &mut consumed,
        domain::fees::ExternalMessageParams {
            gas_limit: U256::zero(),
            max_gas_price: U256::zero(),
        },
        gl_call::On::Finalized,
        super::ConsumeExternalArgs {
            is_deploy: false,
            calldata_length: 0,
        },
    )
    .await
    .unwrap();
    assert_eq!(fees.message_fee.sum(), U256::zero());
    assert_eq!(fees.receipt_fee.sum(), U256::one());
    assert_eq!(consumed, U256::zero());
    assert_eq!(
        shared.data_fees_limit.remaining().await["test"],
        U256::from(9)
    );
}

fn external_message_allocation() -> domain::fees::MessageAllocationNode {
    domain::fees::MessageAllocationNode {
        recipient: None,
        call_key: None,
        budget: Some(U256::from(100)),
        on: gl_call::On::Finalized,
        fee_params: domain::fees::MessageAllocationNodeParams::External(
            domain::fees::ExternalMessageParams {
                gas_limit: U256::zero(),
                max_gas_price: U256::zero(),
            },
        ),
        children_budget: U256::zero(),
        subtree: bytes::Bytes::new(),
    }
}

fn internal_message_allocation() -> domain::fees::MessageAllocationNode {
    domain::fees::MessageAllocationNode {
        recipient: None,
        call_key: None,
        budget: Some(U256::from(100)),
        on: gl_call::On::Finalized,
        fee_params: domain::fees::MessageAllocationNodeParams::Internal(Arc::new(
            domain::fees::InternalMessageParams {
                leader_timeunits_allocation: U256::one(),
                validator_timeunits_allocation: U256::one(),
                execution_budget_per_round: U256::one(),
                rotations: vec![U256::one()],
                max_price_gen_per_time_unit: U256::one(),
                storage_fee_max_gas_price: U256::one(),
                receipt_fee_max_gas_price: U256::one(),
            },
        )),
        children_budget: U256::zero(),
        subtree: bytes::Bytes::new(),
    }
}

#[test]
fn internal_allocation_prefers_exact_key_over_earlier_wildcard() {
    let recipient = calldata::Address::from([7; 20]);
    let call_key = abi::CallKey([8; 32]);
    let mut wildcard = internal_message_allocation();
    wildcard.recipient = Some(recipient);
    wildcard.budget = Some(U256::one());
    let mut exact = wildcard.clone();
    exact.call_key = Some(call_key);
    exact.budget = Some(U256::from(2));
    let nodes = vec![wildcard, exact];

    let (matched, _) =
        resolve_internal_allocation(&nodes, gl_call::On::Finalized, recipient, call_key)
            .expect("exact allocation should match");

    assert_eq!(nodes[matched].budget, Some(U256::from(2)));
}

#[test]
fn internal_allocation_keeps_exhausted_exact_key() {
    let recipient = calldata::Address::from([7; 20]);
    let call_key = abi::CallKey([8; 32]);
    let mut wildcard = internal_message_allocation();
    wildcard.recipient = Some(recipient);
    wildcard.budget = Some(U256::one());
    let mut exact = wildcard.clone();
    exact.call_key = Some(call_key);
    exact.budget = Some(U256::zero());
    let nodes = vec![wildcard, exact];

    let (matched, _) =
        resolve_internal_allocation(&nodes, gl_call::On::Finalized, recipient, call_key)
            .expect("exhausted exact allocation should still match");

    assert_eq!(matched, 1);
    assert_eq!(nodes[matched].budget, Some(U256::zero()));
}

#[test]
fn internal_allocation_keeps_uncapped_exact_key() {
    let recipient = calldata::Address::from([7; 20]);
    let call_key = abi::CallKey([8; 32]);
    let wildcard = internal_message_allocation();
    let mut exact = wildcard.clone();
    exact.recipient = Some(recipient);
    exact.call_key = Some(call_key);
    exact.budget = None;
    let nodes = vec![wildcard, exact];
    assert_eq!(
        resolve_internal_allocation(&nodes, gl_call::On::Finalized, recipient, call_key)
            .unwrap()
            .0,
        1
    );
}

#[test]
fn internal_allocation_does_not_search_duplicate_chain_keys_by_phase() {
    let recipient = calldata::Address::from([7; 20]);
    let call_key = abi::CallKey([8; 32]);
    let mut first = internal_message_allocation();
    first.recipient = Some(recipient);
    first.call_key = Some(call_key);
    let mut duplicate = first.clone();
    duplicate.on = gl_call::On::Accepted;
    assert!(resolve_internal_allocation(
        &[first, duplicate],
        gl_call::On::Accepted,
        recipient,
        call_key
    )
    .is_none());
}

#[test]
fn internal_allocation_phase_is_checked_after_key_resolution() {
    let recipient = calldata::Address::from([7; 20]);
    let call_key = abi::CallKey([8; 32]);
    let mut wildcard = internal_message_allocation();
    wildcard.recipient = Some(recipient);
    let mut exact = wildcard.clone();
    exact.call_key = Some(call_key);
    exact.on = gl_call::On::Accepted;
    let nodes = vec![wildcard, exact];

    assert!(
        resolve_internal_allocation(&nodes, gl_call::On::Finalized, recipient, call_key,).is_none()
    );
}

#[test]
fn internal_allocation_selects_phase_within_equal_keys() {
    let recipient = calldata::Address::from([7; 20]);
    let call_key = abi::CallKey([8; 32]);
    let mut finalized = internal_message_allocation();
    finalized.budget = Some(U256::one());
    let mut decided = finalized.clone();
    decided.on = gl_call::On::Accepted;
    decided.budget = Some(U256::from(2));
    let nodes = vec![finalized, decided];

    let (matched, _) =
        resolve_internal_allocation(&nodes, gl_call::On::Accepted, recipient, call_key)
            .expect("decided allocation should match");

    assert_eq!(nodes[matched].budget, Some(U256::from(2)));
}

#[test]
fn external_allocation_candidates_follow_consensus_precedence() {
    let recipient = calldata::Address::from([7; 20]);
    let call_key = abi::CallKey([8; 32]);
    let mut global_wildcard = external_message_allocation();
    global_wildcard.budget = Some(U256::one());
    let mut recipient_wildcard = global_wildcard.clone();
    recipient_wildcard.recipient = Some(recipient);
    recipient_wildcard.budget = Some(U256::from(2));
    let mut exact = recipient_wildcard.clone();
    exact.call_key = Some(call_key);
    exact.budget = Some(U256::from(3));
    let nodes = vec![global_wildcard, recipient_wildcard, exact];

    let candidates = external_allocation_candidates(&nodes, recipient, call_key);
    let budgets = candidates
        .into_iter()
        .map(|index| nodes[index].budget)
        .collect::<Vec<_>>();

    assert_eq!(
        budgets,
        vec![Some(U256::from(3)), Some(U256::from(2)), Some(U256::one())]
    );
}

#[test]
fn external_allocation_candidates_include_zero_budget_nodes() {
    let recipient = calldata::Address::from([7; 20]);
    let call_key = abi::CallKey([8; 32]);
    let mut node = external_message_allocation();
    node.recipient = Some(recipient);
    node.call_key = Some(call_key);
    node.budget = Some(U256::zero());
    let nodes = vec![node];

    assert_eq!(
        external_allocation_candidates(&nodes, recipient, call_key),
        vec![0]
    );
}
