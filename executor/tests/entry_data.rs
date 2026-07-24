use genlayer_calldata::codec::{Decode, HasDeserializer};
use genlayer_sdk::abi;
use genvm::calldata::{self, Value};
use genvm::public_abi;

fn map(entries: &[(&str, Value)]) -> Value {
    Value::Map(
        entries
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect(),
    )
}

fn validate_main(data: &[u8], is_init: bool) -> Result<(), ()> {
    if is_init {
        abi::entry::MainDeployData::validate(data.into_deserializer()).map_err(|_| ())
    } else {
        abi::entry::MainCallData::validate(data.into_deserializer()).map_err(|_| ())
    }
}

fn check(value: &Value, is_init: bool) -> Result<(), ()> {
    validate_main(&calldata::encode(value), is_init)
}

#[test]
fn valid_call_passes() {
    let v = map(&[
        ("", Value::Str("do_thing".to_owned())),
        ("args", Value::Array(vec![Value::Null])),
        ("kwargs", map(&[("k", Value::Bool(true))])),
    ]);
    check(&v, false).unwrap();
}

#[test]
fn unnamed_call_passes() {
    // The `UNNAMED` case: no method name at all.
    check(&map(&[]), false).unwrap();
    check(&map(&[("", Value::Str(String::new()))]), false).unwrap();
}

#[test]
fn partial_shapes_pass() {
    check(&map(&[("args", Value::Array(vec![]))]), false).unwrap();
    check(&map(&[("kwargs", map(&[]))]), false).unwrap();
}

#[test]
fn special_method_passes() {
    let v = map(&[("", Value::Str("#get-schema".to_owned()))]);
    check(&v, false).unwrap();
}

#[test]
fn deploy_without_method_passes() {
    check(&map(&[]), true).unwrap();
    check(&map(&[("args", Value::Array(vec![Value::Null]))]), true).unwrap();
}

#[test]
fn deploy_with_method_is_invalid() {
    // The deployment shape has no `""` key at all (spec: abi, method calling
    // convention). Today `resolve_method` silently ignores it on init.
    let v = map(&[("", Value::Str("#get-schema".to_owned()))]);
    assert!(check(&v, true).is_err());
    let v = map(&[("", Value::Str(String::new()))]);
    assert!(check(&v, true).is_err());
}

#[test]
fn non_map_payload_is_invalid() {
    // `MainCalldata::parse` currently accepts each of these as a default
    // (unnamed, no-argument) call.
    for v in [
        Value::Null,
        Value::Bool(true),
        Value::Str("do_thing".to_owned()),
        Value::Array(vec![]),
    ] {
        assert!(check(&v, false).is_err(), "{v:?}");
    }
}

#[test]
fn unknown_key_is_invalid() {
    // The key set is closed: tolerating extras would let two byte-different
    // payloads describe the same call and hash differently.
    let v = map(&[
        ("", Value::Str("do_thing".to_owned())),
        ("extra", Value::Null),
    ]);
    assert!(check(&v, false).is_err());
    assert!(check(&map(&[("method", Value::Null)]), false).is_err());
}

#[test]
fn wrong_typed_method_is_invalid() {
    for v in [Value::Null, Value::Bool(false), Value::Array(vec![])] {
        assert!(check(&map(&[("", v.clone())]), false).is_err(), "{v:?}");
    }
}

#[test]
fn wrong_typed_args_is_invalid() {
    // Silently dropped by both SDKs today; reaches the Python splat unchecked.
    for v in [Value::Null, Value::Str("x".to_owned()), map(&[])] {
        assert!(check(&map(&[("args", v.clone())]), false).is_err(), "{v:?}");
    }
}

#[test]
fn wrong_typed_kwargs_is_invalid() {
    for v in [
        Value::Null,
        Value::Str("x".to_owned()),
        Value::Array(vec![]),
    ] {
        assert!(
            check(&map(&[("kwargs", v.clone())]), false).is_err(),
            "{v:?}"
        );
    }
}

#[test]
fn undecodable_payload_is_invalid() {
    assert!(validate_main(&[0xff, 0xff], false).is_err(),);
    assert!(validate_main(&[], false).is_err());
}

#[test]
fn trailing_bytes_are_invalid() {
    let mut data = calldata::encode(&map(&[]));
    data.push(0);
    assert!(validate_main(&data, false).is_err());
}

#[test]
fn payload_exceeding_depth_limit_is_invalid() {
    let mut v = Value::Null;
    for _ in 0..200 {
        v = Value::Array(vec![v]);
    }
    let data = calldata::encode(&map(&[("args", Value::Array(vec![v]))]));
    assert!(validate_main(&data, false).is_err());
}
