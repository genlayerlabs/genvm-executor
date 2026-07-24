//! Backward-compat / round-trip coverage for the runner-visibility fields
//! (`custom_runners`, and `RunNondet::runner`) added to `Sandbox` / `RunNondet`.

use bytes::Bytes;
use genlayer_sdk::abi::gl_call::Message;
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

/// A `Sandbox` map produced before the field existed (no `custom_runners` key)
/// decodes with `None` -- the "inherit the parent's whole set" default.
#[test]
fn sandbox_old_encoding_defaults_to_none() {
    // Mirrors the pre-feature `Sandbox` variant (same wire name and fields), so
    // encoding it yields exactly what an old SDK would emit.
    #[derive(calldata::Encode)]
    enum OldMessage {
        #[allow(dead_code)]
        Sandbox {
            data: Bytes,
            runner: String,
            allow_write_storage: bool,
            allow_send_messages: bool,
            allow_register_runners: bool,
        },
    }

    let old = OldMessage::Sandbox {
        data: Bytes::from_static(b"payload"),
        runner: "contract".to_owned(),
        allow_write_storage: true,
        allow_send_messages: false,
        allow_register_runners: true,
    };

    let decoded: Message = Decode::decode(BinaryDeserializer::new(&encode!(old))).unwrap();

    match decoded {
        Message::Sandbox {
            custom_runners,
            allow_write_storage,
            ..
        } => {
            assert!(custom_runners.is_none(), "absent field must decode as None");
            assert!(allow_write_storage);
        }
        other => panic!("expected Sandbox, got {other:?}"),
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
        allow_register_runners: false,
        custom_runners: Some(list.clone()),
    };

    let decoded: Message = Decode::decode(BinaryDeserializer::new(&encode!(msg))).unwrap();

    match decoded {
        Message::Sandbox { custom_runners, .. } => assert_eq!(custom_runners, Some(list)),
        other => panic!("expected Sandbox, got {other:?}"),
    }
}

/// A `RunNondet` map produced before the fields existed decodes with both new
/// optional fields defaulting to `None`.
#[test]
fn run_nondet_old_encoding_defaults_to_none() {
    #[derive(calldata::Encode)]
    enum OldMessage {
        #[allow(dead_code)]
        RunNondet {
            data_leader: Bytes,
            data_validator: Bytes,
        },
    }

    let old = OldMessage::RunNondet {
        data_leader: Bytes::from_static(b"l"),
        data_validator: Bytes::from_static(b"v"),
    };

    let decoded: Message = Decode::decode(BinaryDeserializer::new(&encode!(old))).unwrap();

    match decoded {
        Message::RunNondet {
            runner,
            custom_runners,
            ..
        } => {
            assert!(runner.is_none(), "absent runner must decode as None");
            assert!(custom_runners.is_none(), "absent list must decode as None");
        }
        other => panic!("expected RunNondet, got {other:?}"),
    }
}

/// A `RunNondet` carrying both new fields round-trips.
#[test]
fn run_nondet_new_fields_round_trip() {
    let list = vec!["custom:cccc".to_owned()];
    let msg = Message::RunNondet {
        data_leader: Bytes::from_static(b"l"),
        data_validator: Bytes::from_static(b"v"),
        runner: Some("custom:cccc".to_owned()),
        custom_runners: Some(list.clone()),
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
