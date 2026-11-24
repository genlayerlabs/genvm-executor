use std::collections::BTreeMap;

use genlayer_calldata::elide::USER_STR_LIMIT;
use genlayer_calldata::{Decode, Value, codec};

fn decode_err<T: codec::Decode + std::fmt::Debug>(val: Value) -> String {
    T::decode(codec::ValueDeserializer(val))
        .unwrap_err()
        .to_string()
}

/// A message may carry its own text plus 1 cut copy of each untrusted value
fn assert_bounded(msg: &str) {
    assert!(msg.len() < 2 * USER_STR_LIMIT + 100, "{} bytes", msg.len());
    assert!(msg.contains("..."), "not cut: {msg}");
}

fn huge() -> String {
    "z".repeat(1 << 20)
}

#[derive(Debug, Decode)]
struct Named {
    #[allow(dead_code)]
    x: i32,
}

#[derive(Debug, Decode)]
enum External {
    #[allow(dead_code)]
    Wrap(u32),
}

#[derive(Debug, Decode)]
#[calldata(untagged)]
enum Untagged {
    #[allow(dead_code)]
    Num(u32),
}

#[test]
fn an_unknown_field_name_is_cut() {
    let msg = decode_err::<Named>(Value::Map(BTreeMap::from([
        ("x".into(), Value::from(1i32)),
        (huge(), Value::Null),
    ])));

    assert!(msg.starts_with("unknown field `zzz"), "{msg}");
    assert_bounded(&msg);
}

#[test]
fn an_unknown_variant_name_is_cut() {
    let msg = decode_err::<External>(Value::Map(BTreeMap::from([(huge(), Value::Null)])));

    assert!(msg.starts_with("unknown variant `zzz"), "{msg}");
    assert_bounded(&msg);
}

#[test]
fn an_unmatched_untagged_value_is_cut() {
    let msg = decode_err::<Untagged>(Value::Str(huge()));

    assert_bounded(&msg);
}

#[test]
fn a_short_name_is_kept_whole() {
    let msg = decode_err::<Named>(Value::Map(BTreeMap::from([
        ("x".into(), Value::from(1i32)),
        ("extra".into(), Value::Null),
    ])));

    assert_eq!(msg, "unknown field `extra`");
}
