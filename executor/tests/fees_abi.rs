use genvm::domain::fees::{
    ExternalMessageParams, MessageAllocationNode, MessageAllocationNodeParams, CALL_KEY_WILDCARD,
};
use primitive_types::U256;

fn word(buf: &[u8], index: usize) -> U256 {
    U256::from_big_endian(&buf[index * 32..index * 32 + 32])
}

fn node(budget: u64, children: Vec<MessageAllocationNode>) -> MessageAllocationNode {
    MessageAllocationNode {
        recipient: Some(genlayer_sdk::calldata::Address::from([budget as u8; 20])),
        call_key: None,
        budget: U256::from(budget),
        on: genlayer_sdk::abi::gl_call::On::Finalized,
        fee_params: MessageAllocationNodeParams::External(ExternalMessageParams {
            gas_limit: U256::one(),
            max_gas_price: U256::one(),
        }),
        children,
    }
}

fn element_index(encoded: &[u8], index: usize) -> usize {
    let heads_base = 2 * 32;
    (heads_base + word(encoded, 2 + index).as_usize()) / 32
}

#[test]
fn subtree_uses_raw_array_transport_and_wildcard() {
    let encoded = node(7, vec![]).abi_encode();
    let root = element_index(&encoded, 0);

    assert_eq!(word(&encoded, 0), U256::from(0x20), "array offset");
    assert_eq!(word(&encoded, 1), U256::one(), "array node count");
    assert_eq!(word(&encoded, root + 2), U256::MAX, "root sentinel");
    assert_eq!(
        word(&encoded, root + 4),
        U256::from_big_endian(&CALL_KEY_WILDCARD.0),
        "call-key wildcard"
    );
}

#[test]
fn subtree_contains_matched_node_then_descendants_in_bfs_order() {
    let root = node(10, vec![node(20, vec![node(40, vec![])]), node(30, vec![])]);
    let encoded = root.abi_encode();

    assert_eq!(word(&encoded, 1), U256::from(4));
    let budgets = (0..4)
        .map(|index| word(&encoded, element_index(&encoded, index) + 5))
        .collect::<Vec<_>>();
    assert_eq!(budgets, [10, 20, 30, 40].map(U256::from), "BFS node order");
    assert_eq!(
        word(&encoded, element_index(&encoded, 3) + 2),
        U256::one(),
        "grandchild parent index"
    );
}
