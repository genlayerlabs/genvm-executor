use genlayer_sdk::calldata::{self, codec::Decode as _};
use genvm_modules_interfaces::fees::{
    InternalMessageParams, MessageAllocationNode, MessageAllocationNodeParams,
};
use primitive_types::U256;

#[test]
fn flat_allocation_preserves_opaque_payload_and_requires_descendant_budget() {
    let allocation = MessageAllocationNode {
        recipient: None,
        call_key: None,
        budget: Some(U256::from(100)),
        on: genvm_modules_interfaces::On::Decided,
        fee_params: MessageAllocationNodeParams::Internal(std::sync::Arc::new(
            InternalMessageParams {
                leader_timeunits_allocation: U256::one(),
                validator_timeunits_allocation: U256::one(),
                execution_budget_per_round: U256::one(),
                rotations: vec![U256::zero()],
                max_price_gen_per_time_unit: U256::one(),
                storage_fee_max_gas_price: U256::one(),
                receipt_fee_max_gas_price: U256::one(),
            },
        )),
        children_budget: U256::from(60),
        subtree: vec![0, 255, 7, 0, 128].into(),
    };
    let value = calldata::to_value(&allocation);
    let decoded =
        MessageAllocationNode::decode(calldata::codec::ValueDeserializer(value.clone())).unwrap();
    assert_eq!(decoded.children_budget, allocation.children_budget);
    assert_eq!(decoded.subtree, allocation.subtree);
    assert_eq!(decoded.budget, allocation.budget);

    for budget in [Some(U256::zero()), None] {
        let mut allocation = allocation.clone();
        allocation.budget = budget;
        let decoded = MessageAllocationNode::decode(calldata::codec::ValueDeserializer(
            calldata::to_value(&allocation),
        ))
        .unwrap();
        assert_eq!(decoded.budget, budget);
        let decoded: MessageAllocationNode =
            serde_json::from_value(serde_json::to_value(&allocation).unwrap()).unwrap();
        assert_eq!(decoded.budget, budget);
    }

    for field in ["children_budget", "subtree"] {
        let calldata::Value::Map(mut fields) = value.clone() else {
            panic!("allocation must encode as a map")
        };
        fields.remove(field);
        let error = MessageAllocationNode::decode(calldata::codec::ValueDeserializer(
            calldata::Value::Map(fields),
        ))
        .unwrap_err();
        assert!(
            error.to_string().contains(field),
            "unexpected error: {error}"
        );
    }
}
