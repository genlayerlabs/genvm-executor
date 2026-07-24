//! `#[calldata(option_as_absence)]`: an `Option<T>` field whose `None` is a
//! *missing map key*, not a present `null`. Decode never errors on absence;
//! encode omits the key and leaves it out of the map length. This mirrors the
//! calling-convention shape hand-written in `executor/src/entry_data.rs`.

use genlayer_calldata::{Decode, Encode, Encoder, Value, codec};

fn encode_to_value<T>(val: &T) -> Value
where
    T: for<'a> codec::Encode<&'a mut Vec<u8>, Error = std::convert::Infallible>,
{
    let mut buf = Vec::new();
    codec::Encode::encode(val, &mut Encoder::new(&mut buf)).unwrap();
    genlayer_calldata::decode(&buf).unwrap()
}

fn try_decode<T: codec::Decode>(val: Value) -> Result<T, codec::DecodeError> {
    T::decode(codec::ValueDeserializer(val))
}

fn decode<T: codec::Decode>(val: Value) -> T {
    try_decode(val).unwrap()
}

fn map(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::Map(entries.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

/// A mandatory field alongside two `option_as_absence` fields -- the entry-data
/// shape: the wire value of an optional field is the inner `T`, never an
/// `Option`/`null`.
#[derive(Debug, PartialEq, Encode, Decode)]
struct Entry {
    id: u32,
    #[calldata(option_as_absence)]
    name: Option<String>,
    #[calldata(option_as_absence)]
    args: Option<Vec<u32>>,
}

// -- decode -----------------------------------------------------------

#[test]
fn absent_keys_decode_to_none() {
    let got: Entry = decode(map([("id", Value::from(1u32))]));
    assert_eq!(
        got,
        Entry {
            id: 1,
            name: None,
            args: None,
        }
    );
}

#[test]
fn present_keys_decode_to_some_inner() {
    let got: Entry = decode(map([
        ("id", Value::from(7u32)),
        ("name", Value::from("hi")),
        (
            "args",
            Value::Array(vec![Value::from(1u32), Value::from(2u32)]),
        ),
    ]));
    assert_eq!(
        got,
        Entry {
            id: 7,
            name: Some("hi".into()),
            args: Some(vec![1, 2]),
        }
    );
}

#[test]
fn one_present_one_absent() {
    let got: Entry = decode(map([
        ("id", Value::from(3u32)),
        ("name", Value::from("only-name")),
    ]));
    assert_eq!(
        got,
        Entry {
            id: 3,
            name: Some("only-name".into()),
            args: None,
        }
    );
}

/// Absence is legal, but the mandatory field is still required.
#[test]
fn mandatory_field_still_required() {
    let err = try_decode::<Entry>(map([("name", Value::from("x"))])).unwrap_err();
    assert!(
        matches!(err, codec::DecodeError::FieldMissing("id")),
        "expected FieldMissing(\"id\"), got {err:?}"
    );
}

/// A present key means present-*with-value*: a `null` payload is decoded as the
/// inner `T` (here `String`) and rejected, not silently treated as absence.
#[test]
fn present_null_is_not_absence() {
    let err =
        try_decode::<Entry>(map([("id", Value::from(1u32)), ("name", Value::Null)])).unwrap_err();
    assert!(
        matches!(err, codec::DecodeError::UnexpectedKind(_)),
        "expected a kind mismatch decoding null as String, got {err:?}"
    );
}

// -- encode -----------------------------------------------------------

#[test]
fn none_omits_key_and_shrinks_map() {
    let v = encode_to_value(&Entry {
        id: 9,
        name: None,
        args: None,
    });
    assert_eq!(v, map([("id", Value::from(9u32))]));
}

#[test]
fn some_emits_inner_value() {
    let v = encode_to_value(&Entry {
        id: 4,
        name: Some("n".into()),
        args: Some(vec![5]),
    });
    assert_eq!(
        v,
        map([
            ("args", Value::Array(vec![Value::from(5u32)])),
            ("id", Value::from(4u32)),
            ("name", Value::from("n")),
        ])
    );
}

// -- roundtrip --------------------------------------------------------

#[test]
fn roundtrips() {
    for e in [
        Entry {
            id: 0,
            name: None,
            args: None,
        },
        Entry {
            id: 1,
            name: Some("a".into()),
            args: None,
        },
        Entry {
            id: 2,
            name: None,
            args: Some(vec![1, 2, 3]),
        },
        Entry {
            id: 3,
            name: Some("b".into()),
            args: Some(vec![]),
        },
    ] {
        let back: Entry = decode(encode_to_value(&e));
        assert_eq!(back, e);
    }
}
