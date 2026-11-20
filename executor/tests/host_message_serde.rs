use genvm::host::message::{AccountAddress, SlotID};

fn decode<T: serde::de::DeserializeOwned>(json: &str) -> Result<T, String> {
    serde_json::from_str(json).map_err(|e| e.to_string())
}

#[test]
fn bytes_serialize_as_padded_standard_base64() {
    let mut addr = [0u8; 20];
    addr[0] = 0xfb;
    addr[19] = 0xff;
    assert_eq!(
        serde_json::to_string(&AccountAddress(addr)).unwrap(),
        r#""+wAAAAAAAAAAAAAAAAAAAAAAAP8=""#
    );
    assert_eq!(
        serde_json::to_string(&SlotID([0x3f; 32])).unwrap(),
        r#""Pz8/Pz8/Pz8/Pz8/Pz8/Pz8/Pz8/Pz8/Pz8/Pz8/Pz8=""#
    );
}

#[test]
fn bytes_round_trip() {
    let slot = SlotID(std::array::from_fn(|i| i as u8 * 7));
    let json = serde_json::to_string(&slot).unwrap();
    assert_eq!(decode::<SlotID>(&json).unwrap(), slot);
}

#[test]
fn padding_is_optional_when_decoding() {
    assert_eq!(
        decode::<AccountAddress>(r#""+wAAAAAAAAAAAAAAAAAAAAAAAP8""#).unwrap(),
        decode::<AccountAddress>(r#""+wAAAAAAAAAAAAAAAAAAAAAAAP8=""#).unwrap()
    );
}

#[test]
fn wrong_length_is_rejected() {
    let err = decode::<AccountAddress>(r#""AAAA""#).unwrap_err();
    assert!(
        err.contains("Can't convert a Byte Vector of length 3 to the output type."),
        "unexpected error: {err}"
    );
}

#[test]
fn non_standard_alphabet_is_rejected() {
    let err = decode::<AccountAddress>(r#""-wAAAAAAAAAAAAAAAAAAAAAAAP8=""#).unwrap_err();
    assert!(err.contains("Invalid symbol"), "unexpected error: {err}");
}

#[test]
fn non_string_is_rejected() {
    let err = decode::<SlotID>("[0, 1]").unwrap_err();
    assert!(
        err.contains("a base64 encoded string"),
        "unexpected error: {err}"
    );
}
