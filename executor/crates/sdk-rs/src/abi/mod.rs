//! Application Binary Interface for GenLayer contracts.
//!
//! This module provides the types, traits, and functions needed to build
//! GenLayer intelligent contracts in Rust.
//!
//! - [`consts`]: Auto-generated constants (EntryKind, ResultCode, etc.)
//! - [`entry`]: Contract entry point handling and the Contract trait
//! - [`gl_call`]: Message types for gl_call operations
//! - [`wasi`]: WASI bindings for storage, balance, and gl_call

use genlayer_calldata as calldata;

#[cfg(feature = "arbitrary")]
pub(crate) mod arb;

pub mod consts;
pub mod entry;
pub mod fees;
pub mod gl_call;

#[cfg(feature = "wasi")]
pub mod wasi;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    calldata::Encode,
    calldata::Decode,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct CallKey(
    #[calldata(
        serialize_with = ::genlayer_calldata::codec::as_bytes::serialize,
        deserialize_with = ::genlayer_calldata::codec::as_bytes::deserialize,
    )]
    pub [u8; 32],
);

#[cfg(test)]
mod storage_type_tests {
    use super::consts::StorageType;
    use serde::Deserialize;

    #[test]
    #[allow(deprecated)]
    fn decided_names_preserve_wire_values_and_legacy_aliases() {
        fn legacy_match(value: StorageType) -> u8 {
            match value {
                StorageType::Default => 0,
                StorageType::LatestFinal => 1,
                StorageType::LatestNonFinal => 2,
            }
        }

        assert_eq!(StorageType::LatestFinalized.value(), 1);
        assert_eq!(StorageType::LatestDecided.value(), 2);
        assert_eq!(StorageType::try_from(1), Ok(StorageType::LatestFinalized));
        assert_eq!(StorageType::try_from(2), Ok(StorageType::LatestDecided));
        assert_eq!(StorageType::LatestFinal, StorageType::LatestFinalized);
        assert_eq!(StorageType::LatestNonFinal, StorageType::LatestDecided);
        assert_eq!(legacy_match(StorageType::LatestDecided), 2);
        let legacy_name =
            serde::de::value::StrDeserializer::<serde::de::value::Error>::new("LatestNonFinal");
        assert_eq!(
            StorageType::deserialize(legacy_name).unwrap(),
            StorageType::LatestDecided
        );
    }
}

impl CallKey {
    pub const DEPLOY: CallKey = CallKey([0u8; 32]);
    pub const UNNAMED: CallKey = CallKey([0u8; 32]);

    pub fn for_method(name: &str) -> CallKey {
        use sha3::Digest;

        let name = name.as_bytes();
        let mut call_key = [0u8; 32];

        if name.len() < 32 {
            call_key[..name.len()].copy_from_slice(name);
        } else {
            let mut hasher = sha3::Keccak256::new();
            hasher.update(name);
            call_key.copy_from_slice(&hasher.finalize());
            call_key[31] |= 1;
        }

        CallKey(call_key)
    }

    pub fn as_u256(&self) -> primitive_types::U256 {
        primitive_types::U256::from_big_endian(&self.0)
    }

    pub fn from_u256(value: primitive_types::U256) -> CallKey {
        CallKey(value.to_big_endian())
    }
}
