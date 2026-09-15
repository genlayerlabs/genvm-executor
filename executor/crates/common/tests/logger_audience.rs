use genvm_common::logger::{
    is_reserved_kv_key, log_into_buffer, Audience, Callsite, Capture, Error, ILogger, Level,
    LogIntoBufferConfig, Record, DEFAULT_BYTES_LIMIT,
};
use genvm_common::{
    log_error_into, log_info_into, log_static_into, log_warn_into, log_with_level_into,
};

/// Formats every record it is given, so a test can assert the emitted line
#[derive(Default)]
struct Capturing(std::sync::Mutex<Vec<String>>);

impl ILogger for Capturing {
    fn try_log(&self, record: Record<'_>) -> Result<(), Error> {
        let mut buf = Vec::new();
        log_into_buffer(&mut buf, record, LogIntoBufferConfig::default())?;
        self.0.lock().unwrap().push(String::from_utf8(buf).unwrap());

        Ok(())
    }

    fn enabled(&self, _callsite: Callsite) -> bool {
        true
    }
}

impl Capturing {
    fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

fn render_line(
    callsite: Callsite,
    kv: &[(&'static str, Capture<'_>)],
    bytes_limit: usize,
) -> String {
    let mut buf = Vec::new();
    log_into_buffer(
        &mut buf,
        Record {
            callsite,
            args: format_args!("hello"),
            kv,
            file: file!(),
            line: line!(),
        },
        LogIntoBufferConfig { bytes_limit },
    )
    .unwrap();

    String::from_utf8(buf).unwrap()
}

#[test]
fn audience_follows_level_in_json() {
    let line = render_line(
        Callsite {
            level: Level::Warn,
            audience: Audience::Operator,
            target: "tgt",
        },
        &[],
        DEFAULT_BYTES_LIMIT,
    );

    assert!(
        line.starts_with(r#"{"level":"warn","audience":"operator","target":"tgt""#),
        "unexpected line: {line}"
    );
}

#[test]
fn introspector_is_the_default_audience() {
    let line = render_line(
        Callsite {
            level: Level::Info,
            audience: Audience::default(),
            target: "tgt",
        },
        &[],
        DEFAULT_BYTES_LIMIT,
    );

    assert!(
        line.contains(r#""audience":"introspector""#),
        "unexpected line: {line}"
    );
}

#[test]
fn user_records_clamp_bytes_even_in_trace_mode() {
    let data = vec![b'a'; 1000];

    let user = render_line(
        Callsite {
            level: Level::Info,
            audience: Audience::User,
            target: "tgt",
        },
        &[("data", Capture::Bytes(&data))],
        usize::MAX,
    );
    assert!(user.contains("..."), "not truncated: {user}");
    assert!(user.len() < 500, "too long: {user}");

    let introspector = render_line(
        Callsite {
            level: Level::Info,
            audience: Audience::Introspector,
            target: "tgt",
        },
        &[("data", Capture::Bytes(&data))],
        usize::MAX,
    );
    assert!(
        !introspector.contains("..."),
        "must not be truncated: {introspector}"
    );
}
#[test]
fn every_grammar_form_tags_the_record_it_emits() {
    let logger = Capturing::default();

    log_warn_into!(@operator, &logger, space_left = 11; "not enough memory");
    log_error_into!(@user, &logger; "no kv, just message");
    log_info_into!(@user, &logger, error:err = std::fmt::Error; "with a kv");
    log_info_into!(&logger, x = 1; "no audience given");
    log_static_into!(Level::Debug, @introspector, &logger; "explicit default");
    log_with_level_into!(Level::Info, @(Audience::User), &logger, x = 1; "runtime audience");
    log_with_level_into!(Level::Info, @(Audience::Operator), &logger; "runtime audience, no kv");

    let lines = logger.lines();
    let expected = [
        (
            r#""level":"warn","audience":"operator""#,
            "not enough memory",
        ),
        (
            r#""level":"error","audience":"user""#,
            "no kv, just message",
        ),
        (r#""level":"info","audience":"user""#, "with a kv"),
        (
            r#""level":"info","audience":"introspector""#,
            "no audience given",
        ),
        (
            r#""level":"debug","audience":"introspector""#,
            "explicit default",
        ),
        (r#""level":"info","audience":"user""#, "runtime audience"),
        (
            r#""level":"info","audience":"operator""#,
            "runtime audience, no kv",
        ),
    ];

    assert_eq!(lines.len(), expected.len(), "emitted {lines:?}");
    for (line, (tag, message)) in lines.iter().zip(expected) {
        assert!(line.contains(tag), "expected {tag} in {line}");
        assert!(line.contains(message), "expected {message} in {line}");
    }
}

/// The macros reject a reserved key at compile time; a hand-built record (the panic
/// hook, the manager's own sink logger) is caught here instead - loudly in dev, by
/// skipping the key in release, so the line can never carry two of them.
#[cfg(debug_assertions)]
#[test]
#[should_panic = "reserved kv key"]
fn a_hand_built_reserved_kv_key_panics_in_dev() {
    render_line(
        Callsite {
            level: Level::Info,
            audience: Audience::Operator,
            target: "tgt",
        },
        &[("audience", Capture::Str("user"))],
        DEFAULT_BYTES_LIMIT,
    );
}

#[cfg(not(debug_assertions))]
#[test]
fn a_hand_built_reserved_kv_key_is_skipped() {
    let line = render_line(
        Callsite {
            level: Level::Info,
            audience: Audience::Operator,
            target: "tgt",
        },
        &[("audience", Capture::Str("user"))],
        DEFAULT_BYTES_LIMIT,
    );

    assert_eq!(
        line.matches(r#""audience":"#).count(),
        1,
        "duplicate key in {line}"
    );
    assert!(
        line.contains(r#""audience":"operator""#),
        "the callsite audience must win: {line}"
    );
}

#[test]
fn only_audience_is_a_reserved_kv_key() {
    assert!(is_reserved_kv_key("audience"));

    for key in ["level", "message", "target", "file", "ts", "audiences", ""] {
        assert!(!is_reserved_kv_key(key), "{key} must stay usable");
    }
}

#[test]
fn audience_parses_case_insensitively() {
    use std::str::FromStr;

    assert_eq!(Audience::from_str("USER"), Ok(Audience::User));
    assert_eq!(Audience::from_str("Operator"), Ok(Audience::Operator));
    assert_eq!(Audience::from_str("nobody"), Err(()));
}
