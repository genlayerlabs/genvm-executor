use genvm::calldata::Address;
use genvm::runners::{self, ChainState, Id, IdUnresolved};
use genvm::SlotID;

fn test_addr() -> String {
    format!("0x{}1", "0".repeat(39))
}

fn test_slot() -> String {
    genlayer_sdk::gvm32::encode(&[0x11u8; 32])
}

#[test]
fn chain_canonical_uses_checksum_address() {
    let address = Address::from([
        0x5a, 0xae, 0xb6, 0x05, 0x3f, 0x3e, 0x94, 0xc9, 0xb9, 0xa0, 0x9f, 0x33, 0x66, 0x94, 0x35,
        0xe7, 0xef, 0x1b, 0xea, 0xed,
    ]);

    let id = Id::Chain {
        address,
        on: ChainState::Decided,
        slot: SlotID::ZERO,
    };

    let expected = format!(
        "chain:0x{}:d:{}",
        std::str::from_utf8(&address.checksum_hex()).unwrap(),
        "0".repeat(52),
    );
    assert_eq!(id.canonical().as_str(), expected.as_str());
    assert!(expected.contains("5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed"));
}

#[test]
fn chain_canonical_chars_are_distinct() {
    let chars: Vec<char> = [
        ChainState::Decided,
        ChainState::Finalized,
        ChainState::Deploy,
    ]
    .into_iter()
    .map(|on| {
        let id = Id::Chain {
            address: Address::from([0u8; 20]),
            on,
            slot: SlotID::ZERO,
        };
        id.canonical()
            .as_str()
            .split(':')
            .nth(2)
            .unwrap()
            .chars()
            .next()
            .unwrap()
    })
    .collect();

    assert_eq!(chars, vec!['d', 'f', 'i']);
}

#[test]
fn parse_runner_id_forms() {
    assert!(matches!(
        runners::parse_runner_id("contract"),
        Some(IdUnresolved::Contract)
    ));
    assert!(matches!(
        runners::parse_runner_id("py-genlayer:test"),
        Some(IdUnresolved::Builtin { .. })
    ));
    assert!(matches!(
        runners::parse_runner_id("custom:deadbeef"),
        Some(IdUnresolved::Custom { .. })
    ));

    match runners::parse_runner_id(&format!("chain:{}:f:{}", test_addr(), test_slot())) {
        Some(IdUnresolved::Chain { on, slot, .. }) => {
            assert_eq!(on, Some(ChainState::Finalized));
            assert!(slot.is_some());
        }
        other => panic!("expected Chain, got {other:?}"),
    }
    match runners::parse_runner_id(&format!("chain:{}:d:{}", test_addr(), test_slot())) {
        Some(IdUnresolved::Chain { on, slot, .. }) => {
            assert_eq!(on, Some(ChainState::Decided));
            assert!(slot.is_some());
        }
        other => panic!("expected Chain, got {other:?}"),
    }
}

#[test]
fn parse_runner_id_rejects_malformed() {
    assert!(runners::parse_runner_id("custom:").is_none());
    assert!(runners::parse_runner_id("custom:bad/char").is_none());
    assert!(runners::parse_runner_id(":hash").is_none());
    assert!(runners::parse_runner_id("name:").is_none());
    assert!(runners::parse_runner_id("with space:hash").is_none());

    assert!(runners::parse_runner_id(&format!("chain:0x01:f:{}", test_slot())).is_none());
    assert!(
        runners::parse_runner_id(&format!("chain:{}:f:{}", "0".repeat(40), test_slot())).is_none()
    );
    assert!(runners::parse_runner_id(&format!("chain:{}:f:0x02", test_addr())).is_none());
    assert!(
        runners::parse_runner_id(&format!("chain:{}:x:{}", test_addr(), test_slot())).is_none()
    );
    // `a` was the pre-rename spelling of the decided state and must not resolve.
    assert!(
        runners::parse_runner_id(&format!("chain:{}:a:{}", test_addr(), test_slot())).is_none()
    );
    assert!(
        runners::parse_runner_id(&format!("chain:{}:f:{}:extra", test_addr(), test_slot()))
            .is_none()
    );
    assert!(
        runners::parse_runner_id(&format!("chain:0x{}:f:{}", "z".repeat(40), test_slot()))
            .is_none()
    );
}

#[test]
fn chain_without_slot_id() {
    match runners::parse_runner_id(&format!("chain:{}:d", test_addr())) {
        Some(IdUnresolved::Chain { on, slot, .. }) => {
            assert_eq!(on, Some(ChainState::Decided));
            assert!(slot.is_none(), "slot must be None when omitted");
        }
        other => panic!("expected Chain, got {other:?}"),
    }
}

#[test]
fn chain_without_on_or_slot() {
    match runners::parse_runner_id(&format!("chain:{}", test_addr())) {
        Some(IdUnresolved::Chain { on, slot, .. }) => {
            assert_eq!(on, Some(ChainState::Decided));
            assert!(slot.is_none(), "slot must be None when omitted");
        }
        other => panic!("expected Chain, got {other:?}"),
    }
}

#[test]
fn chain_with_explicit_slot() {
    match runners::parse_runner_id(&format!("chain:{}:f:{}", test_addr(), test_slot())) {
        Some(IdUnresolved::Chain { on, slot, .. }) => {
            assert_eq!(on, Some(ChainState::Finalized));
            assert!(slot.is_some(), "slot must be Some when given");
        }
        other => panic!("expected Chain, got {other:?}"),
    }
}
