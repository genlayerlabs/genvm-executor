use arbitrary::Arbitrary;
use genlayer_calldata::codec::{Decode, HasDeserializer};
use genlayer_sdk::abi;
use genvm::calldata::{self, Value};

fn reference_validate_main(data: &[u8], is_init: bool) -> bool {
    let Ok(Value::Map(map)) = calldata::decode(data) else {
        return false;
    };

    for (key, value) in map {
        match (key.as_str(), value) {
            ("", Value::Str(_)) if !is_init => {}
            ("args", Value::Array(_)) => {}
            ("kwargs", Value::Map(_)) => {}
            _ => return false,
        }
    }

    true
}

fn assert_validate_main_matches_reference(data: &[u8]) {
    let init_ok = abi::entry::MainDeployData::validate(data.into_deserializer()).is_ok();
    let call_ok = abi::entry::MainCallData::validate(data.into_deserializer()).is_ok();

    assert_eq!(
        init_ok,
        reference_validate_main(data, true),
        "validate_main(data, true) must match the materializing reference"
    );
    assert_eq!(
        call_ok,
        reference_validate_main(data, false),
        "validate_main(data, false) must match the materializing reference"
    );
    assert!(
        !init_ok || call_ok,
        "deployment shape must be a subset of call shape"
    );
}

fn main() {
    afl::fuzz!(|data: &[u8]| {
        assert_validate_main_matches_reference(data);

        let mut u = arbitrary::Unstructured::new(data);
        let Ok(value) = Value::arbitrary(&mut u) else {
            return;
        };
        let encoded = calldata::encode(&value);
        assert_validate_main_matches_reference(&encoded);
    });
}
