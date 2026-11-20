use arbitrary::Arbitrary;
use serde::{Deserialize, Serialize};
use sha3::Digest;

/// Padded standard base64; decoding also accepts unpadded input
mod base64_bytes {
    use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
    use base64::Engine as _;

    const ENGINE: GeneralPurpose = GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
    );

    pub fn serialize<S: serde::Serializer, const N: usize>(
        bytes: &[u8; N],
        ser: S,
    ) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&ENGINE.encode(bytes))
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>, const N: usize>(
        de: D,
    ) -> Result<[u8; N], D::Error> {
        struct Visitor<const N: usize>;

        impl<const N: usize> serde::de::Visitor<'_> for Visitor<N> {
            type Value = [u8; N];

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a base64 encoded string")
            }

            fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<[u8; N], E> {
                let bytes = ENGINE.decode(s).map_err(E::custom)?;
                let len = bytes.len();
                bytes.try_into().map_err(|_| {
                    E::custom(format_args!(
                        "Can't convert a Byte Vector of length {len} to the output type."
                    ))
                })
            }
        }

        de.deserialize_str(Visitor::<N>)
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Hash, Copy, Arbitrary)]
pub struct AccountAddress(#[serde(with = "base64_bytes")] pub [u8; 20]);

impl AccountAddress {
    pub fn raw(&self) -> [u8; 20] {
        let AccountAddress(r) = self;
        *r
    }

    pub fn zero() -> Self {
        Self([0; 20])
    }

    pub const fn len() -> usize {
        20
    }
}

#[derive(
    Debug,
    Default,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    Clone,
    Hash,
    Copy,
    PartialOrd,
    Ord,
    Arbitrary,
)]
#[repr(C)]
pub struct SlotID(#[serde(with = "base64_bytes")] pub [u8; 32]);

impl<W: genlayer_sdk::calldata::Writer> genlayer_sdk::calldata::codec::Encode<W> for SlotID {
    type Error = W::Error;

    fn encode(
        &self,
        enc: &mut genlayer_sdk::calldata::Encoder<W>,
    ) -> std::result::Result<(), Self::Error> {
        enc.push_bytes(&self.0)
    }
}

impl SlotID {
    pub const ZERO: SlotID = SlotID([0; 32]);
    pub const SIZE: u32 = 32;

    pub fn raw(&self) -> [u8; 32] {
        let SlotID(r) = self;
        *r
    }

    pub fn zero() -> Self {
        Self([0; 32])
    }

    pub const fn len() -> usize {
        32
    }

    pub fn indirection(&self, off: u32) -> SlotID {
        let mut digest = sha3::Sha3_256::new();
        digest.update(self.0);
        digest.update(off.to_le_bytes());

        let mut ret = Self::ZERO;
        ret.0.copy_from_slice(digest.finalize().as_slice());
        ret
    }

    pub fn from_bytes(data: [u8; 32]) -> Self {
        SlotID(data)
    }
}

impl From<[u8; 32]> for SlotID {
    fn from(value: [u8; 32]) -> Self {
        SlotID(value)
    }
}

impl From<&[u8; 32]> for SlotID {
    fn from(value: &[u8; 32]) -> Self {
        SlotID(*value)
    }
}

impl std::fmt::Display for SlotID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&genlayer_sdk::nix32::encode(&self.0))
    }
}
