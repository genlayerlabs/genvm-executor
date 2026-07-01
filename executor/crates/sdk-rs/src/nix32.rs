//! # Nix base32 encoder/decoder (v0.2.16 runner-hash scheme)
//!
//! Encodes/decodes byte strings exactly the way Nix does (see nixpkgs
//! `libutil/base32.cc`): 5 bits per character, low bit groups first, over the
//! alphabet `0123456789abcdfghijklmnpqrsvwxyz` (the digits `0`-`9` then `a`-`z`
//! with `e`, `o`, `t`, `u` removed). A 32-byte hash becomes 52 characters whose
//! most-significant group carries 4 zero padding bits. Decoding is
//! case-insensitive.
//!
//! This is the scheme the v0.2.16 runner registry (`all.json`/`latest.json`)
//! and the on-disk runner tarball paths use: a runner tar's id is
//! `nix_base32(sha256(tar))`. The executor formats every 32-byte hash with it,
//! so runner resolution and the `check` command agree with what was released.
//!
//! NOTE: this is a genuine Nix base32 (little-endian bit order), NOT the v0.3
//! line's Crockford Base32 (different alphabet and bit order).

/// Nix base32 alphabet: `0`-`9` then `a`-`z` minus `e`, `o`, `t`, `u`.
const ALPHABET: &[u8] = b"0123456789abcdfghijklmnpqrsvwxyz";

fn decode_digit(c: u8) -> Option<u8> {
    let c = c.to_ascii_lowercase();
    ALPHABET.iter().position(|&b| b == c).map(|p| p as u8)
}

/// Encodes a byte slice as a Nix base32 string (lowercase, most-significant
/// group first).
pub fn encode(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    // ceil(bits / 5) characters; a 32-byte hash is exactly 52 chars.
    let nchars = (bytes.len() * 8 - 1) / 5 + 1;
    let mut out = Vec::with_capacity(nchars);
    for n in (0..nchars).rev() {
        let b = n * 5;
        let i = b / 8;
        let j = (b % 8) as u32;
        let lo = (bytes[i] as u16) >> j;
        let hi = bytes.get(i + 1).map_or(0, |&next| (next as u16) << (8 - j));
        out.push(ALPHABET[((lo | hi) & 0x1f) as usize]);
    }
    String::from_utf8(out).expect("ALPHABET is ASCII")
}

/// Reason a [`decode`] call failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// A character outside the Nix base32 alphabet.
    InvalidChar(char),
    /// The most-significant group carried non-zero padding bits, so the input is
    /// not a canonical encoding of any byte sequence. Carries the number of
    /// leftover `bits` and their non-zero `value` to aid debugging.
    NonZeroPadding { bits: u32, value: u32 },
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DecodeError::InvalidChar(c) => write!(f, "invalid nix base32 character `{c}`"),
            DecodeError::NonZeroPadding { bits, value } => {
                write!(f, "non-zero nix base32 padding: {bits} bit(s) = {value:#x}")
            }
        }
    }
}

impl std::error::Error for DecodeError {}

/// Decodes a Nix base32 string. Case-insensitive.
///
/// Fails with [`DecodeError::InvalidChar`] on a character outside the alphabet
/// or [`DecodeError::NonZeroPadding`] on non-zero padding bits (a non-canonical
/// encoding).
pub fn decode(s: &str) -> Result<Vec<u8>, DecodeError> {
    let chars = s.as_bytes();
    let nchars = chars.len();
    let hash_size = nchars * 5 / 8;
    let pad_bits = (nchars * 5 - hash_size * 8) as u32;
    let mut hash = vec![0u8; hash_size];
    for (p, &c) in chars.iter().enumerate() {
        let digit = decode_digit(c).ok_or(DecodeError::InvalidChar(c as char))?;
        // Chars are most-significant group first; char `p` owns group `n`.
        let n = nchars - 1 - p;
        let b = n * 5;
        let i = b / 8;
        let j = (b % 8) as u32;
        let val = (digit as u16) << j;
        let lo = (val & 0xff) as u8;
        let hi = (val >> 8) as u8;
        if i < hash_size {
            hash[i] |= lo;
        } else if lo != 0 {
            return Err(DecodeError::NonZeroPadding {
                bits: pad_bits,
                value: lo as u32,
            });
        }
        if i + 1 < hash_size {
            hash[i + 1] |= hi;
        } else if hi != 0 {
            return Err(DecodeError::NonZeroPadding {
                bits: pad_bits,
                value: hi as u32,
            });
        }
    }
    Ok(hash)
}
