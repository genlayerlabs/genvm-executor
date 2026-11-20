//! [`jiff::Timestamp`] in the RFC 3339 form that GenVM messages carry
//!
//! The grammar, the error kinds and the output are those of chrono 0.4's
//! `DateTime::parse_from_rfc3339` and `to_rfc3339_opts(SecondsFormat::AutoSi, true)`,
//! except that leap seconds and instants outside [`jiff::Timestamp`]'s range are
//! rejected as out of range

use std::fmt::Write as _;

use jiff::Timestamp;

use crate::codec::DecodeError;
use crate::{Encoder, Value, Writer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    OutOfRange,
    Invalid,
    TooShort,
    TooLong,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ParseError::OutOfRange => "input is out of range",
            ParseError::Invalid => "input contains invalid characters",
            ParseError::TooShort => "premature end of input",
            ParseError::TooLong => "trailing input",
        })
    }
}

impl std::error::Error for ParseError {}

fn two_digits(b: &[u8], at: usize) -> Result<u8, ParseError> {
    let digit = |i: usize| match b[i] {
        c @ b'0'..=b'9' => Ok(c - b'0'),
        _ => Err(ParseError::Invalid),
    };
    Ok(digit(at)? * 10 + digit(at + 1)?)
}

fn expect(b: &[u8], at: usize, c: u8) -> Result<(), ParseError> {
    if b[at] == c {
        Ok(())
    } else {
        Err(ParseError::Invalid)
    }
}

/// Up to 9 significant fractional digits, scaled to nanoseconds; further digits are skipped
fn fraction(s: &str) -> Result<(&str, i32), ParseError> {
    let digits = s.bytes().take(9).take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return Err(if s.is_empty() {
            ParseError::TooShort
        } else {
            ParseError::Invalid
        });
    }
    let value: i32 = s[..digits].parse().unwrap();
    let nanos = value * 10_i32.pow(9 - digits as u32);
    Ok((
        s[digits..].trim_start_matches(|c: char| c.is_ascii_digit()),
        nanos,
    ))
}

/// `Z`, `z`, `+HH:MM` or `-HH:MM`, where U+2212 MINUS SIGN also counts as `-`
fn offset(s: &str) -> Result<(&str, i32), ParseError> {
    if let Some(rest) = s.strip_prefix(['Z', 'z']) {
        return Ok((rest, 0));
    }
    let (negative, s) = if let Some(rest) = s.strip_prefix('+') {
        (false, rest)
    } else if let Some(rest) = s.strip_prefix(['-', '\u{2212}']) {
        (true, rest)
    } else if s.is_empty() {
        return Err(ParseError::TooShort);
    } else {
        return Err(ParseError::Invalid);
    };
    let b = s.as_bytes();
    if b.len() < 2 {
        return Err(ParseError::TooShort);
    }
    let hours = i32::from(two_digits(b, 0)?);
    match b.get(2) {
        Some(b':') => {}
        Some(_) => return Err(ParseError::Invalid),
        None => return Err(ParseError::TooShort),
    }
    let minutes = match b.get(3..5) {
        Some(&[m1 @ b'0'..=b'5', m2 @ b'0'..=b'9']) => i32::from((m1 - b'0') * 10 + (m2 - b'0')),
        Some(&[b'6'..=b'9', b'0'..=b'9']) => return Err(ParseError::OutOfRange),
        Some(_) => return Err(ParseError::Invalid),
        None => return Err(ParseError::TooShort),
    };
    let seconds = hours * 3600 + minutes * 60;
    Ok((&s[5..], if negative { -seconds } else { seconds }))
}

pub fn parse(s: &str) -> Result<Timestamp, ParseError> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return Err(ParseError::TooShort);
    }
    let year = i16::from(two_digits(b, 0)?) * 100 + i16::from(two_digits(b, 2)?);
    expect(b, 4, b'-')?;
    let month = two_digits(b, 5)? as i8;
    expect(b, 7, b'-')?;
    let day = two_digits(b, 8)? as i8;
    let date = jiff::civil::Date::new(year, month, day).map_err(|_| ParseError::OutOfRange)?;
    if !matches!(b[10], b'T' | b't' | b' ') {
        return Err(ParseError::Invalid);
    }
    let hour = two_digits(b, 11)?;
    expect(b, 13, b':')?;
    let minute = two_digits(b, 14)?;
    expect(b, 16, b':')?;
    let second = two_digits(b, 17)?;
    let leap = second == 60;

    let (rest, nanos) = if b.get(19) == Some(&b'.') {
        fraction(&s[20..])?
    } else {
        (&s[19..], 0)
    };
    if hour >= 24 || minute >= 60 || second > 60 {
        return Err(ParseError::OutOfRange);
    }

    let (rest, offset_seconds) = offset(rest)?;
    if !rest.is_empty() {
        return Err(ParseError::TooLong);
    }
    // A leap second is refused only here, so that a malformed tail keeps chrono's error
    if offset_seconds.abs() >= 86_400 || leap {
        return Err(ParseError::OutOfRange);
    }

    let time = jiff::civil::Time::new(hour as i8, minute as i8, second as i8, nanos)
        .map_err(|_| ParseError::OutOfRange)?;
    jiff::tz::Offset::from_seconds(offset_seconds)
        .and_then(|offset| offset.to_timestamp(date.to_datetime(time)))
        .map_err(|_| ParseError::OutOfRange)
}

/// UTC with a `Z`, and as few fractional digits of 3, 6 or 9 as are exact
pub fn format(ts: Timestamp) -> String {
    let dt = jiff::tz::Offset::UTC.to_datetime(ts);
    let mut out = String::with_capacity(30);
    let year = dt.year();
    if (0..=9999).contains(&year) {
        write!(out, "{year:04}").unwrap();
    } else {
        write!(out, "{year:+05}").unwrap();
    }
    write!(
        out,
        "-{:02}-{:02}T{:02}:{:02}:{:02}",
        dt.month(),
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second()
    )
    .unwrap();
    let nanos = dt.subsec_nanosecond();
    if nanos == 0 {
    } else if nanos % 1_000_000 == 0 {
        write!(out, ".{:03}", nanos / 1_000_000).unwrap();
    } else if nanos % 1_000 == 0 {
        write!(out, ".{:06}", nanos / 1_000).unwrap();
    } else {
        write!(out, ".{nanos:09}").unwrap();
    }
    out.push('Z');
    out
}

/// Whole seconds since the Unix epoch, rounded towards negative infinity
pub fn unix_seconds(ts: Timestamp) -> i64 {
    ts.as_nanosecond().div_euclid(1_000_000_000) as i64
}

/// Nanoseconds past [`unix_seconds`], always in `0..1_000_000_000`
pub fn unix_subsec_nanos(ts: Timestamp) -> u32 {
    ts.as_nanosecond().rem_euclid(1_000_000_000) as u32
}

pub fn serialize<W: Writer>(ts: &Timestamp, enc: &mut Encoder<W>) -> Result<(), W::Error> {
    enc.push_str(&format(*ts))
}

pub fn deserialize(val: Value) -> Result<Timestamp, DecodeError> {
    let Value::Str(s) = val else {
        return Err(DecodeError::UnexpectedKind(val.kind()));
    };
    parse(&s).map_err(|e| DecodeError::UserError(Box::new(e)))
}

/// `null` for [`None`]
pub mod option {
    use super::*;

    pub fn serialize<W: Writer>(
        ts: &Option<Timestamp>,
        enc: &mut Encoder<W>,
    ) -> Result<(), W::Error> {
        match ts {
            Some(ts) => super::serialize(ts, enc),
            None => enc.push_null(),
        }
    }

    pub fn deserialize(val: Value) -> Result<Option<Timestamp>, DecodeError> {
        match val {
            Value::Null => Ok(None),
            val => super::deserialize(val).map(Some),
        }
    }
}

/// The same string form for `#[serde(with = ...)]`
pub mod serde {
    use super::*;

    pub fn serialize<S: ::serde::Serializer>(ts: &Timestamp, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&format(*ts))
    }

    pub fn deserialize<'de, D: ::serde::Deserializer<'de>>(de: D) -> Result<Timestamp, D::Error> {
        let s = <String as ::serde::Deserialize>::deserialize(de)?;
        parse(&s).map_err(::serde::de::Error::custom)
    }

    /// `null` for [`None`]
    pub mod option {
        use super::*;

        pub fn serialize<S: ::serde::Serializer>(
            ts: &Option<Timestamp>,
            ser: S,
        ) -> Result<S::Ok, S::Error> {
            match ts {
                Some(ts) => ser.serialize_some(&format(*ts)),
                None => ser.serialize_none(),
            }
        }

        pub fn deserialize<'de, D: ::serde::Deserializer<'de>>(
            de: D,
        ) -> Result<Option<Timestamp>, D::Error> {
            let s = <Option<String> as ::serde::Deserialize>::deserialize(de)?;
            s.map(|s| parse(&s).map_err(::serde::de::Error::custom))
                .transpose()
        }
    }
}
