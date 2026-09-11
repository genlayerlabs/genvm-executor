use genlayer_sdk::abi::entry::{MainCallData, MainDeployData};
use genlayer_sdk::calldata::unparsed::{Maybe, Raw};
use genlayer_sdk::calldata::{self, Map, Value};

fn argument_wire(field: &str, payload: &[u8]) -> Vec<u8> {
    calldata::encode_obj(&Map::from([(
        field.to_owned(),
        Raw(payload.to_vec().into()),
    )]))
}

fn assert_rejected(bytes: &[u8]) {
    assert!(calldata::decode_obj::<MainCallData>(bytes).is_err());
    assert!(calldata::decode_obj::<MainDeployData>(bytes).is_err());
}

#[test]
fn flat_arguments_are_retained_as_whole_collections() {
    let args = vec![Value::Null; 16_384];
    let kwargs = (0..1024)
        .map(|i| (format!("k{i}"), Value::Null))
        .collect::<Map<_>>();
    let value = Value::Map(Map::from([
        ("args".to_owned(), Value::Array(args.clone())),
        ("kwargs".to_owned(), Value::Map(kwargs.clone())),
    ]));
    let bytes = calldata::encode(&value);
    let call: MainCallData = calldata::decode_obj(&bytes).unwrap();
    let deploy: MainDeployData = calldata::decode_obj(&bytes).unwrap();

    assert_eq!(calldata::encode_obj(&call), bytes);
    assert_eq!(calldata::encode_obj(&deploy), bytes);
    assert_eq!(call.args, deploy.args);
    assert_eq!(call.kwargs, deploy.kwargs);
    assert_eq!(
        call.args,
        Some(Maybe::Checked(Raw(calldata::encode_obj(&args).into())))
    );
    assert_eq!(
        call.kwargs,
        Some(Maybe::Checked(Raw(calldata::encode_obj(&kwargs).into())))
    );
    assert_eq!(call.args.unwrap().materialize().unwrap(), args);
    assert_eq!(call.kwargs.unwrap().materialize().unwrap(), kwargs);

    let eager: MainCallData = calldata::from_value(value).unwrap();
    assert_eq!(calldata::encode_obj(&eager), bytes);
}

#[test]
fn absent_and_empty_collections_remain_distinct() {
    for has_args in [false, true] {
        for has_kwargs in [false, true] {
            let mut fields = Map::new();
            if has_args {
                fields.insert("args".to_owned(), Value::Array(Vec::new()));
            }
            if has_kwargs {
                fields.insert("kwargs".to_owned(), Value::Map(Map::new()));
            }
            let bytes = calldata::encode(&Value::Map(fields));
            let call: MainCallData = calldata::decode_obj(&bytes).unwrap();
            let deploy: MainDeployData = calldata::decode_obj(&bytes).unwrap();

            assert_eq!(call.args.is_some(), has_args);
            assert_eq!(call.kwargs.is_some(), has_kwargs);
            assert_eq!(call.args, deploy.args);
            assert_eq!(call.kwargs, deploy.kwargs);
            assert_eq!(calldata::encode_obj(&call), bytes);
            assert_eq!(calldata::encode_obj(&deploy), bytes);
        }
    }
}

#[test]
fn deferred_collections_reject_wrong_kinds_and_malformed_values() {
    for field in ["args", "kwargs"] {
        for payload in [Value::Null, Value::Bool(false), Value::Str(String::new())] {
            assert_rejected(&argument_wire(field, &calldata::encode(&payload)));
        }
    }
    assert_rejected(&argument_wire("args", &[0x06])); // Map, not array
    assert_rejected(&argument_wire("kwargs", &[0x05])); // Array, not map
    for payload in [
        &[0x0d][..],         // Truncated array
        &[0x0d, 0x0c, 0xff], // Invalid UTF-8
        &[0x0d, 0x80, 0x00], // Non-minimal integer encoding
        &[0x0d, 0x00, 0x00], // Trailing bytes
    ] {
        assert_rejected(&argument_wire("args", payload));
    }
    for payload in [
        &[0x16, 0x01, b'a', 0x00, 0x01, b'a', 0x00][..], // Duplicate keys
        &[0x16, 0x01, b'b', 0x00, 0x01, b'a', 0x00],     // Unsorted keys
        &[0x0e, 0x01, 0xff, 0x00],                       // Invalid key UTF-8
        &[0x0e, 0x01, b'a'],                             // Missing value
    ] {
        assert_rejected(&argument_wire("kwargs", payload));
    }
}

#[test]
fn deferred_collections_share_the_enclosing_depth_budget() {
    let max_depth = calldata::codec::BinaryDeserializerOptions::default().max_depth;
    for depth in [max_depth - 2, max_depth - 1] {
        let mut value = Value::Null;
        for _ in 0..depth {
            value = Value::Array(vec![value]);
        }
        for (field, collection) in [
            ("args", Value::Array(vec![value.clone()])),
            (
                "kwargs",
                Value::Map(Map::from([("k".to_owned(), value.clone())])),
            ),
        ] {
            let payload = calldata::encode(&collection);
            let bytes = argument_wire(field, &payload);
            if depth == max_depth - 1 {
                assert_rejected(&bytes);
            } else {
                let call: MainCallData = calldata::decode_obj(&bytes).unwrap();
                let deploy: MainDeployData = calldata::decode_obj(&bytes).unwrap();
                assert_eq!(calldata::encode_obj(&call), bytes);
                assert_eq!(calldata::encode_obj(&deploy), bytes);
                if field == "args" {
                    assert_eq!(
                        Value::Array(call.args.unwrap().materialize().unwrap()),
                        collection
                    );
                } else {
                    assert_eq!(
                        Value::Map(call.kwargs.unwrap().materialize().unwrap()),
                        collection
                    );
                }
            }
        }
    }
}
