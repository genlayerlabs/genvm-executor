//! `#[derive(Decode)]` for named fields, driven from the *wire* through
//! `decode_obj`. The other derive tests go through `ValueDeserializer`, which
//! takes a different path in the generated visitor: here every field value is
//! read straight from the byte deserializer, so `Maybe` fields stay deferred.

use genlayer_calldata::unparsed::{Maybe, Raw};
use genlayer_calldata::{Decode, Map, Value, codec, decode_obj, encode};

fn map(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::Map(entries.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

fn wire(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Vec<u8> {
    encode(&map(entries))
}

fn try_decode<T: codec::Decode>(bytes: &[u8]) -> Result<T, codec::DecodeError> {
    decode_obj(bytes)
}

fn decode<T: codec::Decode>(bytes: &[u8]) -> T {
    try_decode(bytes).unwrap()
}

fn payload_value() -> Value {
    Value::Array(vec![Value::Null, Value::from(1u32)])
}

/// The keys a `Record` cannot be decoded without.
fn required_record() -> Vec<(&'static str, Value)> {
    vec![
        ("id", Value::from(1u32)),
        ("n", Value::from("alice")),
        ("tag", Value::from("hi")),
        ("payload", payload_value()),
    ]
}

fn checked(val: &Value) -> Maybe<Value> {
    Maybe::Checked(Raw(encode(val).into()))
}

// -- Types ------------------------------------------------------------

fn default_score() -> u32 {
    42
}

/// A `deserialize_with` that is observable in the result: it uppercases, so a
/// plain `String` decode could not have produced the same value.
fn shout(val: Value) -> Result<String, codec::DecodeError> {
    match val {
        Value::Str(s) => Ok(s.to_uppercase()),
        _ => Err(codec::DecodeError::Unexpected("expected a string to shout")),
    }
}

/// Every named-field attribute at once, plus a deferred payload.
#[derive(Debug, PartialEq, Decode)]
struct Record {
    id: u32,
    #[calldata(rename = "n")]
    name: String,
    #[calldata(default = default_score)]
    score: u32,
    #[calldata(deserialize_with = shout)]
    tag: String,
    #[calldata(option_as_absence)]
    note: Option<String>,
    payload: Maybe<Value>,
}

/// The `MainCallData` shape: the method name rides on the empty key.
#[derive(Debug, PartialEq, Decode)]
struct CallData {
    #[calldata(rename = "", option_as_absence)]
    name: Option<String>,
    #[calldata(option_as_absence)]
    args: Option<Maybe<Vec<Value>>>,
}

#[derive(Debug, PartialEq, Decode)]
struct Empty {}

/// An enum struct variant shares the same generator as a named struct
/// (`option_as_absence` excepted -- it is rejected outside a struct).
#[derive(Debug, PartialEq, Decode)]
enum Event {
    Emit {
        #[calldata(rename = "t")]
        topic: String,
        #[calldata(default = default_score)]
        score: u32,
        #[calldata(deserialize_with = shout)]
        tag: String,
        blob: Maybe<Value>,
    },
}

// -- Happy path -------------------------------------------------------

#[test]
fn named_struct_decodes_from_the_wire() {
    let mut entries = required_record();
    entries.push(("score", Value::from(7u32)));
    entries.push(("note", Value::from("hello")));

    let got: Record = decode(&wire(entries));
    assert_eq!(
        got,
        Record {
            id: 1,
            name: "alice".into(),
            score: 7,
            tag: "HI".into(),
            note: Some("hello".into()),
            payload: checked(&payload_value()),
        }
    );
}

// -- Unknown / missing fields -----------------------------------------

#[test]
fn unknown_field_is_rejected_by_name() {
    let mut entries = required_record();
    entries.push(("z", Value::from(99u32)));

    let err = try_decode::<Record>(&wire(entries)).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unknown field"), "unexpected error: {msg}");
    assert!(msg.contains("`z`"), "should mention field name: {msg}");
}

#[test]
fn missing_required_field_is_rejected() {
    let mut entries = required_record();
    entries.retain(|(k, _)| *k != "id");

    let err = try_decode::<Record>(&wire(entries)).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("field missing"), "unexpected error: {msg}");
    assert!(msg.contains("id"), "should mention field name: {msg}");
}

/// A renamed field is missing under its *wire* name, not its Rust name.
#[test]
fn missing_renamed_field_reports_the_wire_name() {
    let mut entries = required_record();
    entries.retain(|(k, _)| *k != "n");

    let err = try_decode::<Record>(&wire(entries)).unwrap_err();
    assert!(
        matches!(err, codec::DecodeError::FieldMissing("n")),
        "expected FieldMissing(\"n\"), got {err:?}"
    );
}

// -- default ----------------------------------------------------------

#[test]
fn default_fills_an_absent_field() {
    let got: Record = decode(&wire(required_record()));
    assert_eq!(got.score, default_score());
}

#[test]
fn default_is_ignored_when_the_field_is_present() {
    let mut entries = required_record();
    entries.push(("score", Value::from(0u32)));

    let got: Record = decode(&wire(entries));
    assert_eq!(got.score, 0);
}

// -- deserialize_with -------------------------------------------------

#[test]
fn deserialize_with_receives_the_wire_value() {
    let mut entries = required_record();
    entries.retain(|(k, _)| *k != "tag");
    entries.push(("tag", Value::from("shout me")));

    let got: Record = decode(&wire(entries));
    assert_eq!(got.tag, "SHOUT ME");
}

#[test]
fn deserialize_with_error_propagates() {
    let mut entries = required_record();
    entries.retain(|(k, _)| *k != "tag");
    entries.push(("tag", Value::from(1u32)));

    let err = try_decode::<Record>(&wire(entries)).unwrap_err();
    assert!(err.to_string().contains("shout"), "unexpected error: {err}");
}

// -- rename, including the empty key ----------------------------------

#[test]
fn empty_key_carries_the_renamed_field() {
    let bytes = wire([
        ("", Value::from("method")),
        ("args", Value::Array(vec![Value::Null])),
    ]);

    let got: CallData = decode(&bytes);
    assert_eq!(
        got,
        CallData {
            name: Some("method".into()),
            args: Some(Maybe::Checked(Raw(encode(&Value::Array(vec![
                Value::Null
            ]))
            .into()))),
        }
    );
}

#[test]
fn empty_key_absent_is_none() {
    let got: CallData = decode(&wire([("args", Value::Array(vec![]))]));
    assert_eq!(
        got,
        CallData {
            name: None,
            args: Some(Maybe::Checked(Raw(encode(&Value::Array(vec![])).into()))),
        }
    );
}

// -- option_as_absence ------------------------------------------------

#[test]
fn option_as_absence_absent_key_is_none() {
    let got: Record = decode(&wire(required_record()));
    assert_eq!(got.note, None);
}

#[test]
fn option_as_absence_present_key_is_some_inner() {
    let mut entries = required_record();
    entries.push(("note", Value::from("n")));

    let got: Record = decode(&wire(entries));
    assert_eq!(got.note, Some("n".into()));
}

/// A present key means present-*with-value*: `null` is decoded as the inner
/// `String` and rejected, not silently treated as absence.
#[test]
fn option_as_absence_rejects_a_present_null() {
    let mut entries = required_record();
    entries.push(("note", Value::Null));

    let err = try_decode::<Record>(&wire(entries)).unwrap_err();
    assert!(
        matches!(err, codec::DecodeError::UnexpectedKind(_)),
        "expected a kind mismatch decoding null as String, got {err:?}"
    );
}

// -- Deferred fields --------------------------------------------------

#[test]
fn maybe_field_stays_checked_with_the_canonical_bytes() {
    let payload = payload_value();
    let got: Record = decode(&wire(required_record()));

    let Maybe::Checked(Raw(ref raw)) = got.payload else {
        panic!(
            "wire path should keep the field deferred, got {:?}",
            got.payload
        );
    };
    assert_eq!(raw.as_ref(), encode(&payload).as_slice());
    assert_eq!(got.payload.materialize().unwrap(), payload);
}

// -- Empty struct -----------------------------------------------------

#[test]
fn empty_struct_decodes_from_an_empty_map() {
    assert_eq!(decode::<Empty>(&encode(&Value::Map(Map::new()))), Empty {});
}

#[test]
fn empty_struct_rejects_any_key() {
    let err = try_decode::<Empty>(&wire([("x", Value::Null)])).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unknown field"), "unexpected error: {msg}");
    assert!(msg.contains("`x`"), "should mention field name: {msg}");
}

// -- Enum struct variant ----------------------------------------------

fn event_wire(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Vec<u8> {
    encode(&map([("Emit", map(entries))]))
}

#[test]
fn enum_struct_variant_decodes_from_the_wire() {
    let bytes = event_wire([
        ("t", Value::from("Transfer")),
        ("tag", Value::from("hi")),
        ("blob", payload_value()),
    ]);

    assert_eq!(
        decode::<Event>(&bytes),
        Event::Emit {
            topic: "Transfer".into(),
            score: default_score(),
            tag: "HI".into(),
            blob: checked(&payload_value()),
        }
    );
}

#[test]
fn enum_struct_variant_rejects_unknown_field() {
    let bytes = event_wire([
        ("t", Value::from("Transfer")),
        ("tag", Value::from("hi")),
        ("blob", payload_value()),
        ("extra", Value::Null),
    ]);

    let err = try_decode::<Event>(&bytes).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unknown field"), "unexpected error: {msg}");
    assert!(msg.contains("`extra`"), "should mention field name: {msg}");
}

#[test]
fn enum_struct_variant_requires_its_fields() {
    let bytes = event_wire([("t", Value::from("Transfer")), ("blob", payload_value())]);

    let err = try_decode::<Event>(&bytes).unwrap_err();
    assert!(
        matches!(err, codec::DecodeError::FieldMissing("tag")),
        "expected FieldMissing(\"tag\"), got {err:?}"
    );
}
