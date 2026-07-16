//! A 32-byte hash (runner content hash, `custom:` hash, …) with Nix base32 formatting.

/// A 32-byte hash value. Formats (and parses) as Nix base32 (see
/// [`genlayer_sdk::nix32`]).
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    genlayer_calldata::Encode,
    genlayer_calldata::Decode,
    arbitrary::Arbitrary,
)]
#[repr(transparent)]
pub struct Bytes32Hash(
    #[serde(with = "serde_bytes")]
    #[calldata(
        serialize_with = genlayer_calldata::codec::as_bytes::serialize,
        deserialize_with = genlayer_calldata::codec::as_bytes::deserialize,
    )]
    pub [u8; 32],
);

impl Bytes32Hash {
    pub const ZERO: Self = Self([0; 32]);

    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn raw(&self) -> [u8; 32] {
        self.0
    }

    /// The Nix base32 textual form.
    pub fn to_nix32(&self) -> String {
        genlayer_sdk::nix32::encode(&self.0)
    }

    /// Parses a hash from its Nix base32 form.
    ///
    /// Fails with [`FromNix32Error::InvalidEncoding`] for a malformed encoding or
    /// [`FromNix32Error::WrongLength`] for one that does not decode to exactly 32
    /// bytes.
    pub fn from_nix32(s: &str) -> Result<Self, FromNix32Error> {
        let bytes = genlayer_sdk::nix32::decode(s).map_err(FromNix32Error::InvalidEncoding)?;
        let got = bytes.len();
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| FromNix32Error::WrongLength { got })?;
        Ok(Self(bytes))
    }
}

/// Error returned by [`Bytes32Hash::from_nix32`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FromNix32Error {
    /// The string is not valid Nix base32.
    InvalidEncoding(genlayer_sdk::nix32::DecodeError),
    /// Decoded successfully, but not to exactly 32 bytes.
    WrongLength { got: usize },
}

impl std::fmt::Display for FromNix32Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FromNix32Error::InvalidEncoding(e) => write!(f, "invalid nix base32 encoding: {e}"),
            FromNix32Error::WrongLength { got } => {
                write!(f, "expected 32 bytes, got {got}")
            }
        }
    }
}

impl std::error::Error for FromNix32Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FromNix32Error::InvalidEncoding(e) => Some(e),
            FromNix32Error::WrongLength { .. } => None,
        }
    }
}

impl From<[u8; 32]> for Bytes32Hash {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl std::fmt::Display for Bytes32Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_nix32())
    }
}

impl std::fmt::Debug for Bytes32Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("hash#{}", self.to_nix32()))
    }
}
