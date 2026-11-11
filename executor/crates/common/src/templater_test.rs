use super::*;
use std::cell::Cell;

#[test]
fn replace_all_stops_before_visiting_later_replacements() {
    let calls = Cell::new(0);
    let result = replace_all(&DOLLAR_UNFOLDER_RE, "${}${}${}", 3, |_| {
        calls.set(calls.get() + 1);
        Ok("xx")
    });
    assert!(result.is_err());
    assert_eq!(calls.get(), 2);
}

#[test]
fn bounded_append_uses_few_growths_for_many_small_parts() {
    let limit = 600_000;
    let mut output = String::with_capacity(300_000);
    let mut growths = 0;
    for _ in 0..100_000 {
        let capacity = output.capacity();
        push_bounded(&mut output, "abcdef", limit).unwrap();
        growths += usize::from(output.capacity() != capacity);
        assert!(output.capacity() <= limit);
    }
    assert_eq!(output.len(), limit);
    assert!(
        growths <= 2,
        "{growths} capacity growths for 100000 small appends"
    );
    let capacity = output.capacity();
    assert!(push_bounded(&mut output, "x", limit).is_err());
    assert_eq!(output.len(), limit);
    assert_eq!(output.capacity(), capacity);
}

#[test]
fn replace_all_capacity_stays_within_limit_when_growing() {
    let replacement = "x".repeat(600 * 1024);
    let limit = replacement.len() + 1;
    let output = replace_all(&DOLLAR_UNFOLDER_RE, "${}x", limit, |_| Ok(&replacement)).unwrap();
    assert_eq!(output.len(), limit);
    assert!(
        output.capacity() <= limit,
        "capacity={} limit={limit}",
        output.capacity()
    );
}

#[test]
fn replace_all_checks_prefix_replacement_and_tail() {
    for (template, replacement, limit) in [
        ("abcd${}", "", 3),
        ("a${}", "bcd", 3),
        ("${}bcd", "a", 3),
        ("abcd", "", 3),
    ] {
        assert!(replace_all(&DOLLAR_UNFOLDER_RE, template, limit, |_| Ok(replacement)).is_err());
    }
    assert_eq!(
        replace_all(&DOLLAR_UNFOLDER_RE, "a${}c", 3, |_| Ok("b")).unwrap(),
        "abc"
    );
    assert_eq!(
        replace_all(&DOLLAR_UNFOLDER_RE, "${}", 0, |_| Ok("")).unwrap(),
        ""
    );
    assert_eq!(
        replace_all(&DOLLAR_UNFOLDER_RE, "", 0, |_| Ok("")).unwrap(),
        ""
    );
}

#[test]
fn patch_str_limits_output_bytes_not_template_length() {
    let vars = BTreeMap::from([("".to_owned(), "é".to_owned())]);
    assert_eq!(
        patch_str(&vars, "${}", &DOLLAR_UNFOLDER_RE, 2).unwrap(),
        "é"
    );
    assert!(patch_str(&vars, "${}", &DOLLAR_UNFOLDER_RE, 1).is_err());
    assert!(patch_str(&vars, "${}${}", &DOLLAR_UNFOLDER_RE, 3).is_err());
    assert_eq!(
        patch_str(&vars, "${ENV[MISSING]}", &DOLLAR_UNFOLDER_RE, 0).unwrap(),
        ""
    );
    assert!(patch_str(&vars, "${missing}", &DOLLAR_UNFOLDER_RE, usize::MAX).is_err());
}

#[test]
fn patch_str_preserves_borrowed_single_pass_and_trusted_callers() {
    let vars = HashMap::from([("x".to_owned(), "${x}".to_owned())]);
    assert_eq!(
        patch_str(&vars, "${x}", &DOLLAR_UNFOLDER_RE, usize::MAX).unwrap(),
        "${x}"
    );
    assert_eq!(
        patch_json(&vars, serde_json::json!(["${x}"]), &DOLLAR_UNFOLDER_RE).unwrap(),
        serde_json::json!(["${x}"])
    );
    assert_eq!(
        patch_yaml(
            &vars,
            serde_yaml::Value::String("#{x}".to_owned()),
            &HASH_UNFOLDER_RE
        )
        .unwrap(),
        serde_yaml::Value::String("${x}".to_owned())
    );
}
