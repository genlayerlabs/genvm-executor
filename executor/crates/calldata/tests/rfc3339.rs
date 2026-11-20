use genlayer_calldata::Value;
use genlayer_calldata::codec::rfc3339;
use serde::de::IntoDeserializer;

fn reformat(s: &str) -> String {
    rfc3339::format(rfc3339::parse(s).unwrap())
}

fn parse_err(s: &str) -> String {
    rfc3339::parse(s).unwrap_err().to_string()
}

fn serde_parse(s: &str) -> Result<jiff::Timestamp, serde::de::value::Error> {
    rfc3339::serde::deserialize(s.into_deserializer())
}

// Accepted forms

#[test]
fn utc_round_trips_unchanged() {
    for s in [
        "2024-11-26T06:42:42.424242Z",
        "2024-11-26T06:42:42Z",
        "2024-11-26T06:42:42.500Z",
        "2024-11-26T06:42:42.000000001Z",
    ] {
        assert_eq!(reformat(s), s);
    }
}

#[test]
fn separators_and_zulu_are_case_insensitive() {
    for s in [
        "2024-11-26t06:42:42z",
        "2024-11-26 06:42:42Z",
        "2024-11-26T06:42:42+00:00",
        "2024-11-26T06:42:42-00:00",
    ] {
        assert_eq!(reformat(s), "2024-11-26T06:42:42Z", "{s}");
    }
}

#[test]
fn offsets_convert_to_utc() {
    assert_eq!(
        reformat("2024-11-26T06:42:42+01:30"),
        "2024-11-26T05:12:42Z"
    );
    assert_eq!(
        reformat("2024-11-26T06:42:42\u{2212}23:59"),
        "2024-11-27T06:41:42Z"
    );
}

#[test]
fn fraction_keeps_nine_digits_and_shortest_exact_group() {
    assert_eq!(
        reformat("2024-11-26T06:42:42.1Z"),
        "2024-11-26T06:42:42.100Z"
    );
    assert_eq!(
        reformat("2024-11-26T06:42:42.1234Z"),
        "2024-11-26T06:42:42.123400Z"
    );
    assert_eq!(
        reformat("2024-11-26T06:42:42.1234567899999Z"),
        "2024-11-26T06:42:42.123456789Z"
    );
    assert_eq!(reformat("2024-11-26T06:42:42.000Z"), "2024-11-26T06:42:42Z");
}

#[test]
fn years_outside_four_digits_carry_a_sign() {
    assert_eq!(
        reformat("0000-01-01T00:00:00+00:01"),
        "-0001-12-31T23:59:00Z"
    );
}

#[test]
fn unix_parts_round_towards_negative_infinity() {
    let ts = rfc3339::parse("1969-12-31T23:59:59.5Z").unwrap();
    assert_eq!(rfc3339::unix_seconds(ts), -1);
    assert_eq!(rfc3339::unix_subsec_nanos(ts), 500_000_000);
    let ts = rfc3339::parse("1969-12-31T23:59:59Z").unwrap();
    assert_eq!(
        (rfc3339::unix_seconds(ts), rfc3339::unix_subsec_nanos(ts)),
        (-1, 0)
    );
}

#[test]
fn last_supported_instant_is_accepted() {
    assert_eq!(
        reformat("9999-12-30T22:00:00.999999999Z"),
        "9999-12-30T22:00:00.999999999Z"
    );
    assert_eq!(
        reformat("9999-12-31T00:00:00+05:00"),
        "9999-12-30T19:00:00Z"
    );
}

// Codec and serde adapters

#[test]
fn calldata_adapters_take_strings_and_null() {
    let ts = rfc3339::deserialize(Value::Str("2024-11-26T06:42:42Z".to_owned())).unwrap();
    assert_eq!(rfc3339::format(ts), "2024-11-26T06:42:42Z");
    assert_eq!(rfc3339::option::deserialize(Value::Null).unwrap(), None);

    let err = rfc3339::deserialize(Value::Bool(true))
        .unwrap_err()
        .to_string();
    assert!(err.contains("unexpected"), "unexpected error: {err}");
}

#[test]
fn serde_adapter_uses_the_strict_parser() {
    assert_eq!(
        rfc3339::format(serde_parse("2024-11-26T06:42:42+01:00").unwrap()),
        "2024-11-26T05:42:42Z"
    );
    let err = serde_parse("2024-11-26T06:42:42").unwrap_err().to_string();
    assert!(
        err.contains("premature end of input"),
        "unexpected error: {err}"
    );
}

// Rejected forms

#[test]
fn malformed_input_reports_the_chrono_error_kind() {
    assert_eq!(parse_err("2024-11-26T06:42"), "premature end of input");
    assert_eq!(parse_err("2024-11-26T06:42:42"), "premature end of input");
    assert_eq!(
        parse_err("2024-11-26X06:42:42Z"),
        "input contains invalid characters"
    );
    assert_eq!(
        parse_err("2024-11-26T06:42:42.Z"),
        "input contains invalid characters"
    );
    assert_eq!(parse_err("2024-11-26T06:42:42."), "premature end of input");
    assert_eq!(
        parse_err("2024-11-26T06:42:42+0100"),
        "input contains invalid characters"
    );
    assert_eq!(
        parse_err("2024-11-26T06:42:42+01:"),
        "premature end of input"
    );
    assert_eq!(parse_err("2024-11-26T06:42:42Z "), "trailing input");
    assert_eq!(parse_err("2024-02-30T06:42:42Z"), "input is out of range");
    assert_eq!(parse_err("2024-11-26T24:00:00Z"), "input is out of range");
    assert_eq!(
        parse_err("2024-11-26T06:42:42+01:60"),
        "input is out of range"
    );
    assert_eq!(
        parse_err("2024-11-26T06:42:42+24:00"),
        "input is out of range"
    );
}

#[test]
fn leap_second_is_out_of_range() {
    assert_eq!(parse_err("2016-12-31T23:59:60Z"), "input is out of range");
    assert_eq!(parse_err("2016-12-31T23:59:60Zx"), "trailing input");
}

#[test]
fn instant_past_the_supported_range_is_out_of_range() {
    assert_eq!(parse_err("9999-12-31T23:59:59Z"), "input is out of range");
}
