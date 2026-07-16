use genlayer_sdk::nix32::DecodeError;
use genvm_common::bytes32::FromNix32Error;
use genvm_common::Bytes32Hash;

#[test]
fn nix32_round_trip() {
    let bytes = [0xabu8; 32];
    let h = Bytes32Hash::from_bytes(bytes);

    let s = h.to_nix32();
    assert_eq!(s.len(), 52); // ceil(256 / 5)
    assert_eq!(Bytes32Hash::from_nix32(&s), Ok(h));
    assert_eq!(h.to_string(), s); // Display == nix32
    assert_eq!(h.as_bytes(), &bytes);
}

#[test]
fn from_nix32_rejects_invalid() {
    // valid nix base32, but decodes to fewer than 32 bytes
    assert_eq!(
        Bytes32Hash::from_nix32("00"), // decodes to 1 byte
        Err(FromNix32Error::WrongLength { got: 1 })
    );
    assert_eq!(
        Bytes32Hash::from_nix32(""), // decodes to 0 bytes
        Err(FromNix32Error::WrongLength { got: 0 })
    );
    // '!' is not in the alphabet
    assert_eq!(
        Bytes32Hash::from_nix32("!"),
        Err(FromNix32Error::InvalidEncoding(DecodeError::InvalidChar(
            '!'
        )))
    );
    // valid alphabet, but non-zero padding bits
    assert_eq!(
        Bytes32Hash::from_nix32("abc"),
        Err(FromNix32Error::InvalidEncoding(
            DecodeError::NonZeroPadding { bits: 7, value: 40 }
        ))
    );
}
