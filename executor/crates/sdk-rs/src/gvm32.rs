//! # GenVM base32 encoder/decoder (v0.2.16 legacy scheme)
//!
//! Encodes a byte string as one big-endian base-32 number, most-significant
//! digit first, using the alphabet `0123456789abcdfghijklmnpqrsvwxyz` (the
//! digits `0`-`9` then `a`-`z` with `e`, `o`, `t`, `u` removed). A 32-byte hash
//! therefore becomes 52 characters whose leading 4 bits are zero padding.
//! Decoding is case-insensitive and ignores `-`.
//!
//! NOTE: this intentionally differs from the v0.3 line, which uses Crockford
//! Base32 (different alphabet, *trailing* padding, `i`/`l`/`o` aliases). The
//! v0.2.16 runner registry (`all.json`/`latest.json`) and the on-disk runner
//! tarball paths are encoded with THIS scheme, so the legacy executor must
//! match it to resolve runners. The encoding is a pure bijection on the strings
//! that appear there; the intermediate byte values are an executor-internal
//! cache key and never cross the host boundary.

/// v0.2.16 base-32 alphabet: `0`-`9` then `a`-`z` minus `e`, `o`, `t`, `u`.
const ALPHABET: &[u8] = b"0123456789abcdfghijklmnpqrsvwxyz";

fn decode_digit(c: u8) -> Option<u8> {
    let c = c.to_ascii_lowercase();
    ALPHABET.iter().position(|&b| b == c).map(|p| p as u8)
}

/// Encodes a byte slice as a v0.2.16 base-32 string (lowercase, most
/// significant digit first, with leading `0` padding to a fixed width).
pub fn encode(bytes: &[u8]) -> String {
    // ceil(bits / 5) digits; a 32-byte hash is exactly 52 chars.
    let ndigits = (bytes.len() * 8 + 4) / 5;
    // Big-endian magnitude, divided down by 32 in place to peel off digits.
    let mut acc = bytes.to_vec();
    let mut digits_rev = Vec::with_capacity(ndigits);
    while acc.iter().any(|&b| b != 0) {
        let mut rem: u16 = 0;
        for byte in acc.iter_mut() {
            let cur = (rem << 8) | *byte as u16;
            *byte = (cur / 32) as u8;
            rem = cur % 32;
        }
        digits_rev.push(ALPHABET[rem as usize]);
    }
    while digits_rev.len() < ndigits {
        digits_rev.push(ALPHABET[0]);
    }
    digits_rev.reverse();
    String::from_utf8(digits_rev).expect("ALPHABET is ASCII")
}

/// Reason a [`decode`] call failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// A character outside the v0.2.16 base-32 alphabet (after `-` is ignored).
    InvalidChar(char),
    /// The leading group carried non-zero padding bits, so the input is not a
    /// canonical encoding of any byte sequence. Carries the number of leftover
    /// `bits` and their non-zero `value` to aid debugging.
    NonZeroPadding { bits: u32, value: u32 },
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DecodeError::InvalidChar(c) => write!(f, "invalid gvm32 character `{c}`"),
            DecodeError::NonZeroPadding { bits, value } => write!(
                f,
                "non-zero gvm32 leading padding: {bits} bit(s) = {value:#x}"
            ),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Decodes a v0.2.16 base-32 string. Case-insensitive; `-` is ignored.
///
/// Fails with [`DecodeError::InvalidChar`] on a character outside the alphabet
/// or [`DecodeError::NonZeroPadding`] on non-zero leading padding bits.
pub fn decode(s: &str) -> Result<Vec<u8>, DecodeError> {
    let mut nchars = 0usize;
    let mut acc: Vec<u8> = Vec::new(); // big-endian magnitude, acc = acc * 32 + digit
    for &c in s.as_bytes() {
        if c == b'-' {
            continue;
        }
        let digit = decode_digit(c).ok_or(DecodeError::InvalidChar(c as char))?;
        nchars += 1;
        let mut carry = digit as u16;
        for byte in acc.iter_mut().rev() {
            let v = (*byte as u16) * 32 + carry;
            *byte = (v & 0xff) as u8;
            carry = v >> 8;
        }
        while carry > 0 {
            acc.insert(0, (carry & 0xff) as u8);
            carry >>= 8;
        }
    }
    // 5 bits per char, one big number; the value occupies the low
    // floor(bits/8) bytes and the high (nchars*5 - byte_len*8) bits are padding
    // that must be zero.
    let byte_len = nchars * 5 / 8;
    if acc.len() > byte_len {
        let pad_bits = (nchars * 5 - byte_len * 8) as u32;
        let mut value: u32 = 0;
        for &b in &acc[..acc.len() - byte_len] {
            value = (value << 8) | b as u32;
        }
        return Err(DecodeError::NonZeroPadding {
            bits: pad_bits,
            value,
        });
    }
    let mut out = vec![0u8; byte_len - acc.len()];
    out.extend_from_slice(&acc);
    Ok(out)
}
