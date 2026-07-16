use genlayer_sdk::nix32;

#[test]
fn round_trip() {
    let cases = [
        // hex, nix base32 (lowercase)
        (
            "0000000000000000000000000000000000000000000000000000000000000000",
            "0000000000000000000000000000000000000000000000000000",
        ),
        (
            "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
            "07qy3lf1n6hr30bic58l2c91240g1q6hq2qa1440f1h50h1h4080",
        ),
    ];

    for (hex, b32) in cases {
        let bytes = hex::decode(hex).unwrap();
        assert_eq!(nix32::encode(&bytes), b32, "encode {hex}");
        assert_eq!(nix32::decode(b32).unwrap(), bytes, "decode {b32}");
    }
}

#[test]
fn small_widths() {
    assert_eq!(nix32::encode(&[0x00]), "00");
    assert_eq!(nix32::encode(&[0xff]), "7z");
    assert_eq!(nix32::decode("7z").unwrap(), vec![0xffu8]);
}

#[test]
fn empty() {
    assert_eq!(nix32::encode(&[]), "");
    assert_eq!(nix32::decode("").unwrap(), Vec::<u8>::new());
}

#[test]
fn case_insensitive() {
    // uppercase decodes the same (`7z` -> 0xff)
    assert_eq!(nix32::decode("7Z").unwrap(), vec![0xffu8]);
}

#[test]
fn rejects_invalid() {
    use nix32::DecodeError;
    // `e`, `o`, `t`, `u` are not in the alphabet
    assert_eq!(nix32::decode("u"), Err(DecodeError::InvalidChar('u')));
    assert_eq!(nix32::decode("e"), Err(DecodeError::InvalidChar('e')));
    // non-zero padding bits (a non-canonical encoding)
    assert_eq!(
        nix32::decode("80"),
        Err(DecodeError::NonZeroPadding { bits: 2, value: 1 })
    );
}
