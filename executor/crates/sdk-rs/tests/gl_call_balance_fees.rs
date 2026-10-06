//! Backward-compat / round-trip coverage for balance-funded message fields.

use std::collections::BTreeMap;

use genlayer_sdk::abi::CallKey;
use genlayer_sdk::abi::fees::{ExternalMessageParams, InternalMessageParams};
use genlayer_sdk::abi::gl_call::{
    AllocationNode, Descendants, ExternalAllocation, InternalAllocation, Message, On,
};
use genlayer_sdk::calldata::codec::{BinaryDeserializer, Decode, ValueDeserializer};
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
        leader_time_units_allocation: U256::from(5u64),
        validator_time_units_allocation: U256::from(7u64),
        execution_budget_per_round: U256::from(1024u64),
        rotations: vec![U256::from(4u64); 5],
        max_price_gen_per_time_unit: U256::from(9u64),
        storage_fee_max_gas_price: U256::from(20u64),
        receipt_fee_max_gas_price: U256::from(20u64),
    }
}

fn address(last: u8) -> calldata::Address {
    let mut raw = [0; 20];
    raw[19] = last;
    calldata::Address::from(raw)
}

/// A message carrying the two new fields round-trips through the calldata codec.
#[test]
fn emit_internal_message_balance_fields_round_trip() {
    let params = sample_params();
    let msg = Message::EmitInternalMessage {
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
        descendants: Some(Descendants::Open(U256::from(11u64))),
    };

    let decoded: Message = Decode::decode(BinaryDeserializer::new(&encode!(msg))).unwrap();

    match decoded {
        Message::EmitInternalMessage {
            use_balance,
            fee_params,
            descendants,
            value,
            ..
        } => {
            assert!(use_balance);
            assert_eq!(fee_params, Some(params));
            assert_eq!(descendants, Some(Descendants::Open(U256::from(11u64))));
            assert_eq!(value, U256::from(42u64));
        }
        other => panic!("expected EmitInternalMessage, got {other:?}"),
    }
}

#[test]
fn emit_internal_deploy_message_balance_fields_round_trip() {
    let params = sample_params();
    let descendants = Descendants::Pinned(vec![AllocationNode::External(ExternalAllocation {
        parent_index: U256::MAX,
        recipient: address(0x33),
        call_key: CallKey([7; 32]),
        budget: U256::from(200u64),
        fee_params: ExternalMessageParams {
            gas_limit: U256::from(20u64),
            max_gas_price: U256::from(10u64),
        },
    })]);
    let msg = Message::EmitInternalDeployMessage {
        calldata: genlayer_sdk::abi::entry::MainDeployData {
            args: None,
            kwargs: None,
        },
        code: bytes::Bytes::from_static(b"code"),
        value: U256::from(42u64),
        on: On::Decided,
        salt_nonce: U256::from(3u64),
        use_balance: true,
        fee_params: Some(params.clone()),
        descendants: Some(descendants.clone()),
    };

    let decoded: Message = Decode::decode(BinaryDeserializer::new(&encode!(msg))).unwrap();

    match decoded {
        Message::EmitInternalDeployMessage {
            use_balance,
            fee_params,
            descendants: decoded_descendants,
            value,
            on,
            ..
        } => {
            assert!(use_balance);
            assert_eq!(fee_params, Some(params));
            assert_eq!(decoded_descendants, Some(descendants));
            assert_eq!(value, U256::from(42u64));
            assert_eq!(on, On::Decided);
        }
        other => panic!("expected EmitInternalDeployMessage, got {other:?}"),
    }
}

/// An `EmitInternalMessage` map produced before the fields existed (no `use_balance` /
/// `fee_params` keys) decodes with the defaults.
#[test]
fn emit_internal_message_old_encoding_defaults() {
    // Mirrors the pre-feature `EmitInternalMessage` variant (same wire name and fields),
    // so encoding it yields exactly what an old SDK would emit.
    #[derive(calldata::Encode)]
    enum OldMessage {
        #[allow(dead_code)]
        EmitInternalMessage {
            address: calldata::Address,
            calldata: Maybe<Value>,
            value: U256,
            on: On,
        },
    }

    let old = OldMessage::EmitInternalMessage {
        address: calldata::Address::zero(),
        calldata: Maybe::Materialized(Value::Map(Default::default())),
        value: U256::from(1u64),
        on: On::Decided,
    };

    let decoded: Message = Decode::decode(BinaryDeserializer::new(&encode!(old))).unwrap();

    match decoded {
        Message::EmitInternalMessage {
            use_balance,
            fee_params,
            descendants,
            ..
        } => {
            assert!(!use_balance);
            assert!(fee_params.is_none());
            assert!(descendants.is_none());
        }
        other => panic!("expected EmitInternalMessage, got {other:?}"),
    }
}

/// Same backward-compat guarantee for `EmitInternalDeployMessage`.
#[test]
fn emit_internal_deploy_message_old_encoding_defaults() {
    #[derive(calldata::Encode)]
    enum OldMessage {
        #[allow(dead_code)]
        EmitInternalDeployMessage {
            calldata: Maybe<Value>,
            code: Maybe<Value>,
            value: U256,
            on: On,
            salt_nonce: U256,
        },
    }

    let old = OldMessage::EmitInternalDeployMessage {
        calldata: Maybe::Materialized(Value::Map(Default::default())),
        code: Maybe::Materialized(Value::Bytes(b"code".to_vec())),
        value: U256::zero(),
        on: On::Finalized,
        salt_nonce: U256::from(3u64),
    };

    let decoded: Message = Decode::decode(BinaryDeserializer::new(&encode!(old))).unwrap();

    match decoded {
        Message::EmitInternalDeployMessage {
            use_balance,
            fee_params,
            descendants,
            ..
        } => {
            assert!(!use_balance);
            assert!(fee_params.is_none());
            assert!(descendants.is_none());
        }
        other => panic!("expected EmitInternalDeployMessage, got {other:?}"),
    }
}

#[test]
fn emit_internal_message_explicit_null_descendants_decode_as_absent() {
    #[derive(calldata::Encode)]
    enum RawMessage {
        #[allow(dead_code)]
        EmitInternalMessage {
            address: calldata::Address,
            calldata: Maybe<Value>,
            value: U256,
            on: On,
            use_balance: bool,
            fee_params: Option<InternalMessageParams>,
            descendants: Value,
        },
    }

    let raw = RawMessage::EmitInternalMessage {
        address: calldata::Address::zero(),
        calldata: Maybe::Materialized(Value::Map(Default::default())),
        value: U256::zero(),
        on: On::Finalized,
        use_balance: false,
        fee_params: None,
        descendants: Value::Null,
    };

    let decoded: Message = Decode::decode(BinaryDeserializer::new(&encode!(raw))).unwrap();
    match decoded {
        Message::EmitInternalMessage {
            use_balance,
            descendants,
            ..
        } => {
            assert!(!use_balance);
            assert!(descendants.is_none());
        }
        other => panic!("expected EmitInternalMessage, got {other:?}"),
    }
}

#[test]
fn pinned_descendants_round_trip_flat_mixed_nodes() {
    let child = AllocationNode::Internal(InternalAllocation {
        recipient: address(0x22),
        call_key: CallKey::DEPLOY,
        budget: U256::from(20u64),
        fee_params: sample_params(),
        on: On::Decided,
        parent_index: U256::zero(),
    });
    let descendants = Descendants::Pinned(vec![
        AllocationNode::Internal(InternalAllocation {
            recipient: address(0x11),
            call_key: CallKey::for_method("run"),
            budget: U256::from(60u64),
            fee_params: sample_params(),
            on: On::Finalized,
            parent_index: U256::MAX,
        }),
        child,
        AllocationNode::External(ExternalAllocation {
            parent_index: U256::MAX,
            recipient: address(0x33),
            call_key: CallKey([7; 32]),
            budget: U256::from(200u64),
            fee_params: ExternalMessageParams {
                gas_limit: U256::from(20u64),
                max_gas_price: U256::from(10u64),
            },
        }),
    ]);

    let decoded: Descendants =
        Decode::decode(BinaryDeserializer::new(&encode!(descendants.clone()))).unwrap();
    assert_eq!(decoded, descendants);
}

fn external_allocation_value() -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("parent_index".into(), Value::from(U256::MAX)),
        ("recipient".into(), Value::from(address(0x33))),
        ("call_key".into(), Value::Bytes(vec![0; 32])),
        ("budget".into(), Value::from(200u64)),
        (
            "fee_params".into(),
            Value::Map(BTreeMap::from([
                ("gas_limit".into(), Value::from(20u64)),
                ("max_gas_price".into(), Value::from(10u64)),
            ])),
        ),
    ])
}

fn internal_allocation_value() -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("parent_index".into(), Value::from(U256::MAX)),
        ("recipient".into(), Value::from(address(0x11))),
        ("call_key".into(), Value::Bytes(vec![0; 32])),
        ("budget".into(), Value::from(300u64)),
        (
            "fee_params".into(),
            Value::Map(BTreeMap::from([
                ("leader_time_units_allocation".into(), Value::from(5u64)),
                ("validator_time_units_allocation".into(), Value::from(7u64)),
                ("execution_budget_per_round".into(), Value::from(1024u64)),
                ("rotations".into(), Value::Array(vec![Value::from(4u64)])),
                ("max_price_gen_per_time_unit".into(), Value::from(9u64)),
                ("storage_fee_max_gas_price".into(), Value::from(20u64)),
                ("receipt_fee_max_gas_price".into(), Value::from(20u64)),
            ])),
        ),
    ])
}

#[test]
fn descendants_reject_bool_instead_of_open_budget() {
    let err = Descendants::decode(ValueDeserializer(Value::Bool(true))).unwrap_err();
    assert!(
        err.to_string().contains("unexpected"),
        "unexpected error: {err}"
    );
}

#[test]
fn external_allocation_rejects_children_key() {
    let mut fields = external_allocation_value();
    fields.insert("children".into(), Value::Array(vec![]));
    let err = ExternalAllocation::decode(ValueDeserializer(Value::Map(fields))).unwrap_err();
    assert!(
        err.to_string().contains("unknown field"),
        "unexpected error: {err}"
    );
}

#[test]
fn external_allocation_rejects_missing_fee_params() {
    let mut fields = external_allocation_value();
    fields.remove("fee_params");
    let err = ExternalAllocation::decode(ValueDeserializer(Value::Map(fields))).unwrap_err();
    assert!(
        err.to_string().contains("field missing"),
        "unexpected error: {err}"
    );
}

#[test]
fn allocation_node_rejects_internal_shape_missing_on() {
    let result = AllocationNode::decode(ValueDeserializer(Value::Map(internal_allocation_value())));
    assert!(result.is_err());
}

#[test]
fn descendants_decode_limit_precedes_node_decoding() {
    let oversized = Value::Array(vec![Value::Null; 1025]);
    let error =
        Descendants::decode(BinaryDeserializer::new(&calldata::encode(&oversized))).unwrap_err();
    assert!(
        error.to_string().contains("expected 1024 elements"),
        "unexpected error: {error}"
    );
}

#[test]
fn descendants_decode_accepts_limit_and_rejects_recursive_shapes() {
    let nodes = Value::Array(vec![Value::Map(external_allocation_value()); 1024]);
    let decoded = Descendants::decode(BinaryDeserializer::new(&calldata::encode(&nodes))).unwrap();
    let Descendants::Pinned(nodes) = decoded else {
        panic!("expected pinned descendants")
    };
    assert_eq!(nodes.len(), 1024);

    let mut node = internal_allocation_value();
    node.insert("on".into(), Value::Str("finalized".into()));
    node.insert("children".into(), Value::Array(vec![]));
    let error = InternalAllocation::decode(BinaryDeserializer::new(&calldata::encode(
        &Value::Map(node),
    )))
    .unwrap_err();
    assert!(
        error.to_string().contains("unknown field"),
        "unexpected error: {error}"
    );
}
