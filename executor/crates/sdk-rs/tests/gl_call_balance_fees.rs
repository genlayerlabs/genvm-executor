//! Backward-compat / round-trip coverage for the balance-funded fee fields
//! (`use_balance`, `fee_params`) added to `PostMessage` / `DeployContract`.

use genlayer_sdk::abi::fees::InternalMessageParams;
use genlayer_sdk::abi::gl_call::{Message, On};
use genlayer_sdk::calldata::codec::{BinaryDeserializer, Decode};
use genlayer_sdk::calldata::unparsed::Maybe;
use genlayer_sdk::calldata::{self, Encoder, Value};
use primitive_types::U256;

macro_rules! encode {
    ($v:expr) => {{
        let mut buf = Vec::new();
        calldata::codec::Encode::encode(&$v, &mut Encoder::new(&mut buf))
            .unwrap_or_else(|_| unreachable!());
        buf
    }};
}

fn sample_params() -> InternalMessageParams {
    InternalMessageParams {
        leader_timeunits_allocation: U256::from(5u64),
        validator_timeunits_allocation: U256::from(7u64),
        execution_budget_per_round: U256::from(1024u64),
        rotations: vec![U256::from(4u64); 5],
        max_price_gen_per_time_unit: U256::from(9u64),
        storage_fee_max_gas_price: U256::from(20u64),
        receipt_fee_max_gas_price: U256::from(20u64),
    }
}

/// A message carrying the two new fields round-trips through the calldata codec.
#[test]
fn post_message_balance_fields_round_trip() {
    let params = sample_params();
    let msg = Message::PostMessage {
        address: calldata::Address::zero(),
        calldata: genlayer_sdk::abi::entry::MainCallData {
            name: None,
            args: None,
            kwargs: None,
        },
        value: U256::from(42u64),
        on: On::Finalized,
        use_balance: true,
        fee_params: Some(params.clone()),
    };

    let decoded: Message = Decode::decode(BinaryDeserializer::new(&encode!(msg))).unwrap();

    match decoded {
        Message::PostMessage {
            use_balance,
            fee_params,
            value,
            ..
        } => {
            assert!(use_balance);
            assert_eq!(fee_params, Some(params));
            assert_eq!(value, U256::from(42u64));
        }
        other => panic!("expected PostMessage, got {other:?}"),
    }
}

/// A `PostMessage` map produced before the fields existed (no `use_balance` /
/// `fee_params` keys) decodes with the defaults.
#[test]
fn post_message_old_encoding_defaults() {
    // Mirrors the pre-feature `PostMessage` variant (same wire name and fields),
    // so encoding it yields exactly what an old SDK would emit.
    #[derive(calldata::Encode)]
    enum OldMessage {
        #[allow(dead_code)]
        PostMessage {
            address: calldata::Address,
            calldata: Maybe<Value>,
            value: U256,
            on: On,
        },
    }

    let old = OldMessage::PostMessage {
        address: calldata::Address::zero(),
        calldata: Maybe::Materialized(Value::Map(Default::default())),
        value: U256::from(1u64),
        on: On::Accepted,
    };

    let decoded: Message = Decode::decode(BinaryDeserializer::new(&encode!(old))).unwrap();

    match decoded {
        Message::PostMessage {
            use_balance,
            fee_params,
            ..
        } => {
            assert!(!use_balance);
            assert!(fee_params.is_none());
        }
        other => panic!("expected PostMessage, got {other:?}"),
    }
}

/// Same backward-compat guarantee for `DeployContract`.
#[test]
fn deploy_contract_old_encoding_defaults() {
    #[derive(calldata::Encode)]
    enum OldMessage {
        #[allow(dead_code)]
        DeployContract {
            calldata: Maybe<Value>,
            code: Maybe<Value>,
            value: U256,
            on: On,
            salt_nonce: U256,
        },
    }

    let old = OldMessage::DeployContract {
        calldata: Maybe::Materialized(Value::Map(Default::default())),
        code: Maybe::Materialized(Value::Bytes(b"code".to_vec())),
        value: U256::zero(),
        on: On::Finalized,
        salt_nonce: U256::from(3u64),
    };

    let decoded: Message = Decode::decode(BinaryDeserializer::new(&encode!(old))).unwrap();

    match decoded {
        Message::DeployContract {
            use_balance,
            fee_params,
            ..
        } => {
            assert!(!use_balance);
            assert!(fee_params.is_none());
        }
        other => panic!("expected DeployContract, got {other:?}"),
    }
}
