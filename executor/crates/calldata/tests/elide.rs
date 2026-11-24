use genlayer_calldata::elide::{Elided, USER_STR_LIMIT, UserLogStr, user_format};

fn elide(limit: usize, s: &str) -> String {
    Elided::format(limit, format_args!("{s}"))
        .unwrap()
        .to_string()
}

/// Feeds `s` to the formatter in `chunk`-byte writes, as a `Display` impl would
fn elide_chunked(limit: usize, s: &str, chunk: usize) -> String {
    struct Chunked<'a>(&'a str, usize);

    impl std::fmt::Display for Chunked<'_> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let mut rest = self.0;
            while !rest.is_empty() {
                let at = rest.ceil_char_boundary(self.1.min(rest.len()));
                f.write_str(&rest[..at])?;
                rest = &rest[at..];
            }
            Ok(())
        }
    }

    Elided::format(limit, format_args!("{}", Chunked(s, chunk)))
        .unwrap()
        .to_string()
}

#[test]
fn short_strings_pass_unchanged() {
    assert_eq!(elide(8, ""), "");
    assert_eq!(elide(8, "abcdefgh"), "abcdefgh");
}

#[test]
fn long_strings_keep_head_and_tail() {
    assert_eq!(elide(8, "abcdefghi"), "abcd...fghi");
    assert_eq!(elide(8, &"x".repeat(1 << 20)), "xxxx...xxxx");
}

#[test]
fn chunked_writes_match_a_single_write() {
    let s: String = (0..1000)
        .map(|i| char::from(b'a' + (i % 26) as u8))
        .collect();
    for chunk in [1, 3, 7, 64, 129, 2000] {
        for limit in [0, 1, 2, 9, 128, 999, 1000, 1001] {
            assert_eq!(
                elide_chunked(limit, &s, chunk),
                elide(limit, &s),
                "chunk {chunk}, limit {limit}"
            );
        }
    }
}

#[test]
fn cuts_fall_on_char_boundaries() {
    let s = "\u{e9}".repeat(100);
    for limit in 0..20 {
        for chunk in [1, 2, 3, 200] {
            let out = elide_chunked(limit, &s, chunk);
            let kept = out.replace("...", "");
            assert!(kept.len() <= limit, "limit {limit}: {out}");
            assert!(kept.chars().all(|c| c == '\u{e9}'), "limit {limit}: {out}");
        }
    }
}

#[test]
fn a_string_at_the_limit_is_not_cut_even_across_a_char_boundary() {
    // the head ends mid-char, so the tail must take the bytes the head left out
    let s = format!("a{}", "\u{e9}".repeat(4));
    assert_eq!(s.len(), 9);
    assert_eq!(elide(9, &s), s);
    assert_eq!(elide_chunked(9, &s, 1), s);
}

#[test]
fn an_unbounded_limit_keeps_everything() {
    let s = "y".repeat(10_000);
    assert_eq!(elide_chunked(usize::MAX, &s, 7), s);
}

#[test]
fn user_log_str_matches_the_formatter() {
    for len in [0, USER_STR_LIMIT, USER_STR_LIMIT + 1, 1 << 20] {
        let s = "\u{e9}".repeat(len / 2) + &"z".repeat(len % 2);
        assert_eq!(
            UserLogStr(&s).to_string(),
            elide(USER_STR_LIMIT, &s),
            "len {len}"
        );
        assert_eq!(user_format(format_args!("{s}")), elide(USER_STR_LIMIT, &s));
    }
}
