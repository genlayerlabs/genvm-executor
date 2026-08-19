//! Round-trip and rejection coverage for the `gl_call` fields that carry
//! runner visibility (`custom_runners`, `RunNondet::runner`), error catching
//! and `changes_on_error`.

use bytes::Bytes;
use genlayer_sdk::abi::gl_call::{ChangesOnError, Message};
use genlayer_sdk::calldata::codec::{BinaryDeserializer, Decode};
use genlayer_sdk::calldata::{self, Encoder};

macro_rules! encode {
    ($v:expr) => {{
        let mut buf = Vec::new();
        calldata::codec::Encode::encode(&$v, &mut Encoder::new(&mut buf))
            .unwrap_or_else(|_| unreachable!());
        buf
    }};
}

/// `allow_register_runners` is gone from the wire, not merely ignored: an SDK
/// still sending it is refused rather than silently running with a permission
/// this line no longer models.
#[test]
fn sandbox_rejects_the_removed_register_runners_field() {
    #[derive(calldata::Encode)]
    enum WithRemovedField {
        #[allow(dead_code)]
        Sandbox {
            data: Bytes,
            runner: String,
            allow_write_storage: bool,
            allow_send_messages: bool,
            allow_register_runners: bool,
        },
    }

    let msg = WithRemovedField::Sandbox {
        data: Bytes::from_static(b"payload"),
        runner: "contract".to_owned(),
        allow_write_storage: false,
        allow_send_messages: false,
        allow_register_runners: true,
    };

    let decoded: Result<Message, _> = Decode::decode(BinaryDeserializer::new(&encode!(msg)));

    assert!(decoded.is_err(), "expected a decode error, got {decoded:?}");
}

/// `CallContract` carries the VM error opt-in.
#[test]
fn call_contract_catch_vm_error_round_trips() {
    let msg = Message::CallContract {
        address: calldata::Address::from([0u8; 20]),
        calldata: genlayer_sdk::abi::entry::MainCallData {
            name: Some("f".to_owned()),
            args: None,
            kwargs: None,
        },
        state: genlayer_sdk::abi::consts::StorageType::Default,
        catch_vm_error: true,
    };

    let decoded: Message = Decode::decode(BinaryDeserializer::new(&encode!(msg))).unwrap();

    match decoded {
        Message::CallContract { catch_vm_error, .. } => assert!(catch_vm_error),
        other => panic!("expected CallContract, got {other:?}"),
    }
}

/// A `Sandbox` carrying an explicit visibility list round-trips.
#[test]
fn sandbox_custom_runners_round_trip() {
    let list = vec!["custom:aaaa".to_owned(), "custom:bbbb".to_owned()];
    let msg = Message::Sandbox {
        data: Bytes::from_static(b"x"),
        runner: "contract".to_owned(),
        allow_write_storage: false,
        allow_send_messages: false,
        custom_runners: Some(list.clone()),
        changes_on_error: ChangesOnError::Inherit,
    };

    let decoded: Message = Decode::decode(BinaryDeserializer::new(&encode!(msg))).unwrap();

    match decoded {
        Message::Sandbox {
            custom_runners,
            changes_on_error,
            ..
        } => {
            assert_eq!(custom_runners, Some(list));
            assert_eq!(changes_on_error, ChangesOnError::Inherit);
        }
        other => panic!("expected Sandbox, got {other:?}"),
    }
}

/// `inherit` is the only value the wire admits: anything else is a malformed
/// message, not a silently ignored one.
#[test]
fn sandbox_rejects_unknown_changes_on_error() {
    #[derive(calldata::Encode)]
    enum WithBadValue {
        #[allow(dead_code)]
        Sandbox {
            data: Bytes,
            runner: String,
            allow_write_storage: bool,
            allow_send_messages: bool,
            custom_runners: Option<Vec<String>>,
            changes_on_error: String,
        },
    }

    let msg = WithBadValue::Sandbox {
        data: Bytes::from_static(b"payload"),
        runner: "contract".to_owned(),
        allow_write_storage: false,
        allow_send_messages: false,
        custom_runners: None,
        changes_on_error: "revert".to_owned(),
    };

    let decoded: Result<Message, _> = Decode::decode(BinaryDeserializer::new(&encode!(msg)));

    assert!(decoded.is_err(), "expected a decode error, got {decoded:?}");
}

/// A `RunNondet` carrying an explicit runner and visibility list round-trips.
#[test]
fn run_nondet_runner_fields_round_trip() {
    let list = vec!["custom:cccc".to_owned()];
    let msg = Message::RunNondet {
        data_leader: Bytes::from_static(b"l"),
        data_validator: Bytes::from_static(b"v"),
        runner: Some("custom:cccc".to_owned()),
        custom_runners: Some(list.clone()),
        catch_vm_error: false,
    };

    let decoded: Message = Decode::decode(BinaryDeserializer::new(&encode!(msg))).unwrap();

    match decoded {
        Message::RunNondet {
            runner,
            custom_runners,
            ..
        } => {
            assert_eq!(runner, Some("custom:cccc".to_owned()));
            assert_eq!(custom_runners, Some(list));
        }
        other => panic!("expected RunNondet, got {other:?}"),
    }
}
