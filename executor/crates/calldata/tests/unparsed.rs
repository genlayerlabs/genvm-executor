use bytes::Bytes;
use genlayer_calldata::unparsed::{Maybe, Raw};
use genlayer_calldata::{
    Decode, Encode, Map, Value, codec, decode_obj, encode, encode_obj, from_value,
};

/// Max container depth of a default `BinaryDeserializer`.
fn max_depth() -> usize {
    codec::BinaryDeserializerOptions::default().max_depth
}

fn sample_value() -> Value {
    Value::Array(vec![Value::Null, Value::Null, Value::Null])
}

/// `n` nested arrays around a `Null`, i.e. exactly `n` levels of depth.
fn nested_arrays(n: usize) -> Value {
    let mut val = Value::Null;
    for _ in 0..n {
        val = Value::Array(vec![val]);
    }
    val
}

/// `Action::Return` carrying a payload `depth` arrays deep.
fn action_bytes(depth: usize) -> Vec<u8> {
    encode_obj(&Action::Return(Maybe::Materialized(nested_arrays(depth))))
}

// -- Types ------------------------------------------------------------
//
// The enum structure (which variant, the `topics`) is decoded eagerly, while
// the bulky `Value` / `Map` payloads are kept as validated raw bytes until a
// consumer actually needs them.

#[derive(Debug, Encode, Decode)]
enum Action {
    Return(Maybe<Value>),
    EmitEvent {
        topics: Vec<Bytes>,
        blob: Maybe<Map<Value>>,
    },
}

#[derive(Debug, Encode, Decode)]
struct Envelope {
    tag: u64,
    payload: Maybe<Value>,
}

// -- Maybe: deferred decode -------------------------------------------

#[test]
fn maybe_from_wire_keeps_raw_bytes() {
    let val = sample_value();
    let bytes = encode(&val);

    // Decoding from the wire validates but keeps the raw bytes, not a tree.
    let m: Maybe<Value> = decode_obj(&bytes).unwrap();
    let Maybe::Checked(Raw(ref raw)) = m else {
        panic!("wire path should produce Maybe::Checked, got {m:?}");
    };
    assert_eq!(raw.as_ref(), bytes.as_slice());

    assert_eq!(m.materialize().unwrap(), val);
}

#[test]
fn maybe_from_value_materializes_eagerly() {
    let val = sample_value();

    // Decoding from an in-memory Value has no bytes to defer, so it decodes eagerly.
    let m: Maybe<Value> = from_value(val.clone()).unwrap();
    assert!(
        matches!(m, Maybe::Materialized(_)),
        "value path should materialize eagerly, got {m:?}"
    );

    assert_eq!(m.materialize().unwrap(), val);
}

// -- Maybe: validation against the target type ------------------------

#[test]
fn maybe_validates_target_type_at_decode_time() {
    let bytes = encode(&Value::Str("hello".to_owned()));

    // A string is the right shape: deferred, then materializable.
    let ok: Maybe<String> = decode_obj(&bytes).unwrap();
    assert_eq!(ok.materialize().unwrap(), "hello");

    // A string is not a u64: the payload is rejected up front, before materializing.
    let err = decode_obj::<Maybe<u64>>(&bytes).unwrap_err();
    assert!(err.to_string().contains("str"), "unexpected error: {err}");
}

// -- Raw --------------------------------------------------------------

#[test]
fn raw_from_wire_is_the_original_bytes() {
    let val = sample_value();
    let bytes = encode(&val);

    let raw: Raw = decode_obj(&bytes).unwrap();
    assert_eq!(raw.0.as_ref(), bytes.as_slice());
    assert_eq!(raw.decode_as::<Value>().unwrap(), val);
}

#[test]
fn raw_from_value_is_the_canonical_encoding() {
    let val = sample_value();
    let bytes = encode(&val);

    // From a Value source there are no wire bytes, so Raw is the re-encoding.
    let raw: Raw = from_value(val.clone()).unwrap();
    assert_eq!(raw.0.as_ref(), bytes.as_slice());
    assert_eq!(raw.decode_as::<Value>().unwrap(), val);
}

// -- A host-action enum whose heavy payloads stay deferred ------------

#[test]
fn enum_newtype_variant_defers_payload() {
    let inner = Value::Array(vec![Value::Null, Value::Null]);
    let bytes = encode_obj(&Action::Return(Maybe::Materialized(inner.clone())));

    let Action::Return(payload) = decode_obj::<Action>(&bytes).unwrap() else {
        panic!("expected Return variant");
    };
    assert!(
        matches!(payload, Maybe::Checked(_)),
        "payload should stay deferred after decoding from the wire"
    );
    assert_eq!(payload.materialize().unwrap(), inner);
}

#[test]
fn enum_struct_variant_parses_topics_but_defers_blob() {
    let topics = vec![
        Bytes::from_static(b"Transfer"),
        Bytes::from_static(b"\x01\x02"),
    ];
    let blob = Map::from([
        ("amount".to_owned(), Value::Array(vec![Value::Null])),
        ("from".to_owned(), Value::Null),
    ]);
    let bytes = encode_obj(&Action::EmitEvent {
        topics: topics.clone(),
        blob: Maybe::Materialized(blob.clone()),
    });

    let Action::EmitEvent {
        topics: got_topics,
        blob: got_blob,
    } = decode_obj::<Action>(&bytes).unwrap()
    else {
        panic!("expected EmitEvent variant");
    };

    // `topics` is decoded eagerly, the blob is not.
    assert_eq!(got_topics, topics);
    assert!(
        matches!(got_blob, Maybe::Checked(_)),
        "blob should stay deferred after decoding from the wire"
    );
    assert_eq!(got_blob.materialize().unwrap(), blob);
}

#[test]
fn deferred_enum_re_encodes_to_the_same_bytes() {
    let topics = vec![Bytes::from_static(b"Transfer")];
    let blob = Map::from([("from".to_owned(), Value::Null)]);
    let bytes = encode_obj(&Action::EmitEvent {
        topics: topics.clone(),
        blob: Maybe::Materialized(blob),
    });

    // Decode (payload stays as raw bytes) then re-encode: pass-through is byte-exact.
    let decoded = decode_obj::<Action>(&bytes).unwrap();
    assert_eq!(encode_obj(&decoded), bytes);
}

#[test]
fn enum_rejects_multi_key_map() {
    // An external-tagged enum must be a single-key map; extra keys are rejected.
    let two = Value::Map(Map::from([
        ("Return".to_owned(), Value::Null),
        ("zzz".to_owned(), Value::Null),
    ]));
    let bytes = encode(&two);

    let err = decode_obj::<Action>(&bytes).unwrap_err();
    assert!(
        err.to_string().contains("expected 1"),
        "unexpected error: {err}"
    );
}

// -- Decoder limits still apply to deferred payloads ------------------
//
// Validating a `Maybe` field runs on the enclosing deserializer, so its depth
// budget is shared with the struct/enum that wraps it. Only `materialize`
// starts a fresh deserializer, and thus a fresh budget.

#[test]
fn a_standalone_deferred_payload_gets_the_whole_budget() {
    decode_obj::<Maybe<Value>>(&encode(&nested_arrays(max_depth()))).unwrap();

    let err = decode_obj::<Maybe<Value>>(&encode(&nested_arrays(max_depth() + 1))).unwrap_err();
    assert!(
        err.to_string().contains("exceeded maximum container depth"),
        "unexpected error: {err}"
    );
}

#[test]
fn enclosing_enum_depth_counts_toward_the_deferred_payload() {
    // The enum's own single-key map eats one level of budget, so a payload that
    // is fine on its own is one level too deep once wrapped.
    let err = decode_obj::<Action>(&action_bytes(max_depth())).unwrap_err();
    assert!(
        err.to_string().contains("exceeded maximum container depth"),
        "unexpected error: {err}"
    );

    // One level shallower fits inside the enum.
    decode_obj::<Action>(&action_bytes(max_depth() - 1)).unwrap();
}

#[test]
fn a_derived_struct_shares_its_depth_budget_too() {
    // A named struct is a map on the wire, so it costs the same one level.
    let envelope = |depth| {
        encode_obj(&Envelope {
            tag: 7,
            payload: Maybe::Materialized(nested_arrays(depth)),
        })
    };

    decode_obj::<Envelope>(&envelope(max_depth() - 1)).unwrap();

    let err = decode_obj::<Envelope>(&envelope(max_depth())).unwrap_err();
    assert!(
        err.to_string().contains("exceeded maximum container depth"),
        "unexpected error: {err}"
    );
}

#[test]
fn a_derived_struct_field_stays_deferred() {
    let payload = sample_value();
    let bytes = encode_obj(&Envelope {
        tag: 7,
        payload: Maybe::Materialized(payload.clone()),
    });

    let env = decode_obj::<Envelope>(&bytes).unwrap();
    assert_eq!(env.tag, 7);
    let Maybe::Checked(Raw(ref raw)) = env.payload else {
        panic!("struct field should stay deferred, got {:?}", env.payload);
    };
    assert_eq!(raw.as_ref(), encode(&payload).as_slice());
    assert_eq!(env.payload.materialize().unwrap(), payload);
}

#[test]
fn truncated_deferred_payload_is_rejected_at_decode_time() {
    let bytes = action_bytes(4);
    let truncated = &bytes[..bytes.len() - 1];

    let err = decode_obj::<Action>(truncated).unwrap_err();
    assert!(
        err.to_string().contains("unexpected end"),
        "unexpected error: {err}"
    );
}

#[test]
fn payload_at_the_depth_boundary_stays_checked_and_materializes() {
    let Action::Return(payload) = decode_obj::<Action>(&action_bytes(max_depth() - 1)).unwrap()
    else {
        panic!("expected Return variant");
    };
    assert!(
        matches!(payload, Maybe::Checked(_)),
        "payload should stay deferred, got {payload:?}"
    );

    // `materialize` re-decodes with a fresh deserializer, so the budget resets.
    assert_eq!(
        payload.materialize().unwrap(),
        nested_arrays(max_depth() - 1)
    );
}

#[test]
fn trailing_garbage_after_a_deferred_value_is_rejected() {
    let mut bytes = encode(&sample_value());
    bytes.push(0);

    let err = decode_obj::<Maybe<Value>>(&bytes).unwrap_err();
    assert!(
        err.to_string().contains("unexpected end"),
        "unexpected error: {err}"
    );
}
