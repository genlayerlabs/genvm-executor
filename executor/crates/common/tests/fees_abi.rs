use genvm_modules_interfaces::fees::{
    ExternalMessageParams, InternalMessageParams, MessageAllocationNode,
    MessageAllocationNodeParams,
};
use primitive_types::U256;

/// Concatenates 32-byte big-endian ABI words into one buffer.
fn words(values: &[U256]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(values.len() * 32);
    for v in values {
        buf.extend_from_slice(&v.to_big_endian());
    }
    buf
}

/// Reads the `i`-th 32-byte ABI word from an encoded buffer.
fn word(buf: &[u8], i: usize) -> U256 {
    U256::from_big_endian(&buf[i * 32..i * 32 + 32])
}

fn external_node(
    recipient: Option<[u8; 20]>,
    call_key: Option<[u8; 32]>,
    budget: u64,
    gas_limit: u64,
    max_gas_price: u64,
    children: Vec<MessageAllocationNode>,
) -> MessageAllocationNode {
    MessageAllocationNode {
        recipient: recipient.map(genlayer_sdk::calldata::Address::from),
        call_key: call_key.map(genvm_modules_interfaces::CallKey),
        budget: U256::from(budget),
        // External messages have no acceptance/finalize lifecycle; value is unused.
        on: genvm_modules_interfaces::On::Finalized,
        fee_params: MessageAllocationNodeParams::External(ExternalMessageParams {
            gas_limit: U256::from(gas_limit),
            max_gas_price: U256::from(max_gas_price),
        }),
        children,
    }
}

fn internal_node(
    on: genvm_modules_interfaces::On,
    budget: u64,
    rotations: &[u64],
    children: Vec<MessageAllocationNode>,
) -> MessageAllocationNode {
    MessageAllocationNode {
        recipient: None,
        call_key: None,
        budget: U256::from(budget),
        on,
        fee_params: MessageAllocationNodeParams::Internal(std::sync::Arc::new(
            InternalMessageParams {
                leader_timeunits_allocation: U256::from(1),
                validator_timeunits_allocation: U256::from(2),
                execution_budget_per_round: U256::from(3),
                rotations: rotations.iter().map(|r| U256::from(*r)).collect(),
                max_price_gen_per_time_unit: U256::from(11),
                storage_fee_max_gas_price: U256::from(12),
                receipt_fee_max_gas_price: U256::from(13),
            },
        )),
        children,
    }
}

// -- Exact wire layout -----------------------------------------------

#[test]
fn external_root_node_matches_exact_encoding() {
    let recipient = [0x11u8; 20];
    let root = external_node(Some(recipient), None, 5, 7, 9, vec![]);
    let encoded = root.abi_encode();

    // `abi.encode(MessageAllocationNode[])`: array offset, length, element
    // offset, then the 10-word element tuple
    // (messageType=External, onAcceptance=false, parent=sentinel, recipient,
    // callKey wildcard, budget, feeParams offset, feeParams len, gasLimit, maxGasPrice).
    let expected = words(&[
        U256::from(0x20),                  // offset to array
        U256::from(1),                     // array length
        U256::from(0x20),                  // element[0] head offset
        U256::from(0),                     // messageType = External
        U256::from(0),                     // onAcceptance = false
        U256::MAX,                         // parentIndex = NODE_ROOT_SENTINEL
        U256::from_big_endian(&recipient), // recipient (left-padded)
        U256::from_big_endian(&genvm_modules_interfaces::fees::CALL_KEY_WILDCARD.0),
        U256::from(5),    // budget
        U256::from(0xE0), // feeParams offset (7 head words)
        U256::from(64),   // feeParams bytes length
        U256::from(7),    // gasLimit
        U256::from(9),    // maxGasPrice
    ]);

    assert_eq!(encoded, expected);
}

// -- Tree flattening to parent-pointer form --------------------------

#[test]
fn nested_internal_flattens_with_parent_pointers() {
    let grandchild = external_node(Some([0x44u8; 20]), None, 1, 100, 200, vec![]);
    let first_child = external_node(Some([0x22u8; 20]), None, 2, 100, 200, vec![grandchild]);
    let second_child = external_node(Some([0x33u8; 20]), None, 3, 100, 200, vec![]);
    let root = internal_node(
        genvm_modules_interfaces::On::Decided,
        10,
        &[2, 3],
        vec![first_child, second_child],
    );

    let encoded = root.abi_encode();

    assert_eq!(word(&encoded, 0), U256::from(0x20));
    assert_eq!(word(&encoded, 1), U256::from(4), "four flattened nodes");

    // Heads region begins right after the array length word, and the
    // per-element offsets there are relative to it.
    let heads_base = 2 * 32;
    let element_idx = |index: usize| (heads_base + word(&encoded, 2 + index).as_usize()) / 32;
    let root_idx = element_idx(0);
    let first_child_idx = element_idx(1);
    let second_child_idx = element_idx(2);
    let grandchild_idx = element_idx(3);

    // Root: messageType Internal (1), onAcceptance true, parent = sentinel.
    assert_eq!(
        word(&encoded, root_idx),
        U256::from(1),
        "root messageType Internal"
    );
    assert_eq!(
        word(&encoded, root_idx + 1),
        U256::one(),
        "root onAcceptance"
    );
    assert_eq!(
        word(&encoded, root_idx + 2),
        U256::MAX,
        "root parent = sentinel"
    );

    // Both children precede the grandchild in BFS order.
    assert_eq!(
        word(&encoded, first_child_idx + 2),
        U256::zero(),
        "first child parent index 0"
    );
    assert_eq!(
        word(&encoded, second_child_idx + 2),
        U256::zero(),
        "second child parent index 0"
    );
    assert_eq!(
        word(&encoded, grandchild_idx + 2),
        U256::one(),
        "grandchild parent index 1"
    );
}

// -- Chain-derived feeParams fields ----------------------------------

#[test]
fn internal_params_encode_derived_appeal_rounds() {
    // appealRounds is not stored on the Rust side; it is reconstructed as
    // len(rotations) - 1 when encoding.
    let root = internal_node(
        genvm_modules_interfaces::On::Finalized,
        10,
        &[2, 3, 4],
        vec![],
    );
    let encoded = root.abi_encode();

    // Walk to the feeParams bytes inside the single element.
    let heads_base = 2 * 32;
    let elem_idx = (heads_base + word(&encoded, 2).as_usize()) / 32;
    // element head is 7 words; the feeParams offset word is the 7th (elem_idx + 6).
    let fee_params_len_idx = elem_idx + word(&encoded, elem_idx + 6).as_usize() / 32;
    // feeParams = [len][ abi.encode(InternalMessageParams) ], tuple starts after len.
    let inner_base = fee_params_len_idx + 1;

    // InternalMessageParams tuple (v0.6-dev, 8 head words):
    // [offset][leader][validator][appealRounds][exec][rot off][maxPriceGen]
    // [storageCap][receiptCap][rot len][rot...]
    assert_eq!(
        word(&encoded, inner_base),
        U256::from(0x20),
        "struct offset"
    );
    assert_eq!(word(&encoded, inner_base + 1), U256::from(1), "leader");
    assert_eq!(word(&encoded, inner_base + 2), U256::from(2), "validator");
    assert_eq!(
        word(&encoded, inner_base + 3),
        U256::from(2),
        "appealRounds = 3 - 1"
    );
    assert_eq!(
        word(&encoded, inner_base + 4),
        U256::from(3),
        "execBudgetPerRound"
    );
    assert_eq!(
        word(&encoded, inner_base + 5),
        U256::from(0x100),
        "rotations offset (8 head words)"
    );
    assert_eq!(
        word(&encoded, inner_base + 6),
        U256::from(11),
        "maxPriceGenPerTimeUnit"
    );
    assert_eq!(
        word(&encoded, inner_base + 7),
        U256::from(12),
        "storageFeeMaxGasPrice"
    );
    assert_eq!(
        word(&encoded, inner_base + 8),
        U256::from(13),
        "receiptFeeMaxGasPrice"
    );
    assert_eq!(
        word(&encoded, inner_base + 9),
        U256::from(3),
        "rotations length"
    );
}
