use genvm_common::logger::{
    log_into_buffer, Audience, Callsite, Capture, Level, LogIntoBufferConfig, Record,
    DEFAULT_STR_LIMIT, MAX_RECORD_ITEMS, MAX_SERIALIZATION_DEPTH, USER_BYTES_LIMIT,
};

const USER: Callsite = Callsite {
    level: Level::Info,
    audience: Audience::User,
    target: "tgt",
};

const OPERATOR: Callsite = Callsite {
    audience: Audience::Operator,
    ..USER
};

const TRACE: LogIntoBufferConfig = LogIntoBufferConfig {
    bytes_limit: usize::MAX,
    str_limit: usize::MAX,
};

fn render(
    callsite: Callsite,
    args: std::fmt::Arguments<'_>,
    kv: &[(&'static str, Capture<'_>)],
    conf: LogIntoBufferConfig,
) -> serde_json::Map<String, serde_json::Value> {
    let mut buf = Vec::new();
    log_into_buffer(
        &mut buf,
        Record {
            callsite,
            args,
            kv,
            file: file!(),
            line: line!(),
        },
        conf,
    )
    .unwrap();

    serde_json::from_slice(&buf)
        .unwrap_or_else(|e| panic!("not json ({e}): {}", String::from_utf8_lossy(&buf)))
}

fn str_field<'a>(record: &'a serde_json::Map<String, serde_json::Value>, key: &str) -> &'a str {
    record[key].as_str().unwrap()
}

/// What the logger writes for a string cut to `limit` bytes
fn elided(s: &str, limit: usize) -> String {
    genlayer_calldata::elide::Elided::format(limit, format_args!("{s}"))
        .unwrap()
        .to_string()
}

fn render_operator(
    kv: &[(&'static str, Capture<'_>)],
) -> serde_json::Map<String, serde_json::Value> {
    render(
        OPERATOR,
        format_args!("static"),
        kv,
        LogIntoBufferConfig::default(),
    )
}

/// An error with a source chain of any length
#[derive(Debug)]
struct Linked {
    message: String,
    source: Option<Box<Linked>>,
}

impl Linked {
    fn chain(len: usize) -> Self {
        let mut ret = None;
        for i in (0..len).rev() {
            ret = Some(Box::new(Linked {
                message: format!("link {i}"),
                source: ret,
            }));
        }
        *ret.unwrap()
    }
}

impl std::fmt::Display for Linked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Linked {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_deref().map(|s| s as _)
    }
}

fn count_markers(v: &serde_json::Value) -> usize {
    match v {
        serde_json::Value::String(s) => usize::from(s == "$TooMany"),
        serde_json::Value::Array(a) => a.iter().map(count_markers).sum(),
        serde_json::Value::Object(o) => {
            usize::from(o.contains_key("$TooMany")) + o.values().map(count_markers).sum::<usize>()
        }
        _ => 0,
    }
}

fn count_leaves(v: &serde_json::Value) -> usize {
    match v {
        serde_json::Value::Array(a) => a.iter().map(count_leaves).sum(),
        serde_json::Value::Object(o) => o.values().map(count_leaves).sum(),
        serde_json::Value::String(s) if s == "$TooMany" => 0,
        _ => 1,
    }
}

// -- Strings -----------------------------------------------------------

#[test]
fn user_strings_are_cut_like_bytes_even_in_trace_mode() {
    let long = "z".repeat(1 << 20);
    let err = std::io::Error::other(long.clone());
    let record = render(
        USER,
        format_args!("static"),
        &[
            ("s", Capture::Str(&long)),
            ("d", Capture::Display(&long)),
            ("q", Capture::Debug(&long)),
            ("e", Capture::Error(&err)),
        ],
        TRACE,
    );

    let expected = elided(&long, USER_BYTES_LIMIT);
    assert_eq!(str_field(&record, "s"), expected);
    assert_eq!(str_field(&record, "d"), expected);
    assert_eq!(
        str_field(&record, "q"),
        elided(&format!("{long:?}"), USER_BYTES_LIMIT)
    );
    assert_eq!(record["e"]["message"], expected.as_str());
}

#[test]
fn operator_strings_are_cut_at_the_default_limit() {
    let fits = "a".repeat(DEFAULT_STR_LIMIT);
    let long = "b".repeat(DEFAULT_STR_LIMIT + 1);
    let record = render_operator(&[("fits", Capture::Str(&fits)), ("long", Capture::Str(&long))]);

    assert_eq!(str_field(&record, "fits"), fits);
    assert_eq!(str_field(&record, "long"), elided(&long, DEFAULT_STR_LIMIT));
    assert!(str_field(&record, "long").contains("..."));
}

#[test]
fn trace_mode_keeps_operator_strings_whole() {
    let long = "c".repeat(DEFAULT_STR_LIMIT * 4);
    let record = render(
        OPERATOR,
        format_args!("static"),
        &[("long", Capture::Str(&long))],
        TRACE,
    );

    assert_eq!(str_field(&record, "long"), long);
}

#[test]
fn an_interpolated_message_is_cut_and_a_literal_one_is_not() {
    let long = "m".repeat(1000);
    let interpolated = render(
        USER,
        format_args!("got {long}"),
        &[],
        LogIntoBufferConfig::default(),
    );
    assert_eq!(
        str_field(&interpolated, "message"),
        elided(&format!("got {long}"), USER_BYTES_LIMIT)
    );

    let literal = render(
        USER,
        format_args!(
            "a literal message longer than the 128 bytes a user record keeps per value, \
            which only the callsite could have written, so it is never cut"
        ),
        &[],
        LogIntoBufferConfig::default(),
    );
    let message = str_field(&literal, "message");
    assert!(message.len() > USER_BYTES_LIMIT);
    assert!(!message.contains("..."), "{message}");
}

#[test]
fn a_cut_string_keeps_the_dollar_escape() {
    let long = format!("${}", "d".repeat(1000));
    let record = render(
        USER,
        format_args!("static"),
        &[("s", Capture::Str(&long))],
        LogIntoBufferConfig::default(),
    );

    let s = str_field(&record, "s");
    assert!(s.starts_with("$$d"), "{s}");
}

#[test]
fn multibyte_and_control_strings_stay_valid_json_when_cut() {
    let long = "\u{e9}\u{0}".repeat(1000);
    let record = render(
        USER,
        format_args!("static"),
        &[("s", Capture::Str(&long))],
        LogIntoBufferConfig::default(),
    );

    assert_eq!(str_field(&record, "s"), elided(&long, USER_BYTES_LIMIT));
}

#[test]
fn serialized_strings_and_map_keys_are_cut() {
    let long = "k".repeat(DEFAULT_STR_LIMIT * 2);
    let map = std::collections::BTreeMap::from([(long.clone(), long.clone())]);
    let record = render_operator(&[(
        "map",
        Capture::Serde(&|v| serde::Serialize::serialize(&map, v)),
    )]);

    let expected = elided(&long, DEFAULT_STR_LIMIT);
    assert_eq!(
        record["map"],
        serde_json::json!({ expected.clone(): expected })
    );
}

// -- Item budget -------------------------------------------------------

#[test]
fn small_values_are_untouched_by_the_budget() {
    let tree = vec![vec![1u32; 4]; 4];
    let record = render_operator(&[(
        "tree",
        Capture::Serde(&|v| serde::Serialize::serialize(&tree, v)),
    )]);

    assert_eq!(record["tree"], serde_json::json!(tree));
}

#[test]
fn nested_sequences_share_one_record_budget() {
    let tree = vec![vec![1u32; 100]; 100];
    let record = render_operator(&[(
        "tree",
        Capture::Serde(&|v| serde::Serialize::serialize(&tree, v)),
    )]);

    assert!(count_markers(&record["tree"]) > 0, "{:?}", record["tree"]);
    assert!(count_leaves(&record["tree"]) < MAX_RECORD_ITEMS);
}

#[test]
fn a_map_stops_at_the_budget() {
    let map: std::collections::BTreeMap<String, u32> =
        (0..10_000).map(|i| (format!("key{i:05}"), i)).collect();
    let record = render_operator(&[(
        "map",
        Capture::Serde(&|v| serde::Serialize::serialize(&map, v)),
    )]);

    let map = record["map"].as_object().unwrap();
    assert_eq!(map.get("$TooMany"), Some(&serde_json::Value::Bool(true)));
    assert!(map.len() <= MAX_RECORD_ITEMS, "{} entries", map.len());
}

#[test]
fn every_capture_draws_on_the_same_budget() {
    // each sequence stays under the per-sequence cap, so only the budget can cut
    let half: Vec<u32> = (0..MAX_RECORD_ITEMS as u32 / 2).collect();
    let err = Linked::chain(1);
    let record = render_operator(&[
        (
            "first",
            Capture::Serde(&|v| serde::Serialize::serialize(&half, v)),
        ),
        (
            "second",
            Capture::Serde(&|v| serde::Serialize::serialize(&half, v)),
        ),
        (
            "third",
            Capture::Serde(&|v| serde::Serialize::serialize(&half, v)),
        ),
        ("e", Capture::Error(&err)),
    ]);

    assert_eq!(record["first"], serde_json::json!(half));
    assert_eq!(record["second"], serde_json::json!(half));
    assert_eq!(count_markers(&record["third"]), half.len());
    assert_eq!(record["e"], "$TooMany");
}

#[test]
fn an_error_chain_stops_at_the_depth_limit() {
    let err = Linked::chain(MAX_RECORD_ITEMS * 2);
    let record = render_operator(&[("e", Capture::Error(&err))]);

    let mut links = 0;
    let mut cur = &record["e"];
    while let Some(next) = cur.get("source") {
        links += 1;
        cur = next;
    }
    assert_eq!(cur, "$TooDeep");
    assert_eq!(links, MAX_SERIALIZATION_DEPTH);
}

#[test]
fn a_map_past_the_depth_limit_still_stops_at_the_budget() {
    // parsing would merge the repeated marker keys, so the raw line is measured
    let leaf: serde_json::Map<String, serde_json::Value> = (0..10_000)
        .map(|i| (format!("key{i:05}"), serde_json::Value::from(i)))
        .collect();
    let mut value = serde_json::Value::Object(leaf);
    for depth in 1..=MAX_SERIALIZATION_DEPTH + 2 {
        value = serde_json::Value::Array(vec![value]);
        let mut buf = Vec::new();
        log_into_buffer(
            &mut buf,
            Record {
                callsite: OPERATOR,
                args: format_args!("static"),
                kv: &[(
                    "v",
                    Capture::Serde(&|v| serde::Serialize::serialize(&value, v)),
                )],
                file: file!(),
                line: line!(),
            },
            LogIntoBufferConfig::default(),
        )
        .unwrap();

        assert!(
            buf.len() < 16 * 1024,
            "{} bytes at depth {depth}",
            buf.len()
        );
    }
}

#[test]
fn a_map_key_keeps_the_dollar_escape() {
    let map = std::collections::BTreeMap::from([("$key".to_owned(), 1u32)]);
    let record = render_operator(&[(
        "map",
        Capture::Serde(&|v| serde::Serialize::serialize(&map, v)),
    )]);

    assert_eq!(record["map"], serde_json::json!({ "$$key": 1 }));
}

#[test]
fn collected_strings_are_cut() {
    struct Shown(usize);

    impl serde::Serialize for Shown {
        fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            s.collect_str(&"q".repeat(self.0))
        }
    }

    let value = Shown(DEFAULT_STR_LIMIT * 2);
    let record = render_operator(&[(
        "v",
        Capture::Serde(&|v| serde::Serialize::serialize(&value, v)),
    )]);

    assert_eq!(
        str_field(&record, "v"),
        elided(&"q".repeat(DEFAULT_STR_LIMIT * 2), DEFAULT_STR_LIMIT)
    );
}

#[test]
fn a_huge_integer_is_shown_by_its_width() {
    // the logger tells a value apart by its type only where serde hands it over
    // unreferenced, as it does for a struct field
    #[derive(serde::Serialize)]
    struct Field {
        n: num_bigint::BigInt,
    }

    let small = Field {
        n: num_bigint::BigInt::from(1) << 255u32,
    };
    let huge = Field {
        n: -(num_bigint::BigInt::from(1) << 1_000_000u32),
    };
    let record = render_operator(&[
        (
            "small",
            Capture::Serde(&|v| serde::Serialize::serialize(&small, v)),
        ),
        (
            "huge",
            Capture::Serde(&|v| serde::Serialize::serialize(&huge, v)),
        ),
    ]);

    // serde_json parses the 77 digit number as a float
    assert_eq!(record["small"]["n"].as_f64(), Some(2f64.powi(255)));
    assert_eq!(record["huge"]["n"], "-<1000001 bit integer>");
}

#[test]
fn the_deepest_record_still_parses_as_json() {
    #[derive(serde::Serialize)]
    enum Nest {
        Tuple(Box<Nest>, u8),
        Struct { inner: Box<Nest> },
        Leaf,
    }

    let mut tree = Nest::Leaf;
    for i in 0..200 {
        tree = if i % 2 == 0 {
            Nest::Tuple(Box::new(tree), 0)
        } else {
            Nest::Struct {
                inner: Box::new(tree),
            }
        };
    }

    // `render` parses with serde_json's default recursion limit of 128
    let record = render_operator(&[(
        "tree",
        Capture::Serde(&|v| serde::Serialize::serialize(&tree, v)),
    )]);
    assert!(record["tree"].to_string().contains("$TooDeep"));
}

#[test]
fn anyhow_causes_draw_on_the_budget() {
    let mut err = anyhow::anyhow!("root");
    for i in 0..MAX_RECORD_ITEMS * 2 {
        err = err.context(format!("context {i}"));
    }
    let record = render_operator(&[("e", Capture::Anyhow(&err))]);

    let causes = record["e"]["causes"].as_array().unwrap();
    assert_eq!(causes.len(), MAX_RECORD_ITEMS + 1);
    assert_eq!(causes[MAX_RECORD_ITEMS], "$TooMany");
}
