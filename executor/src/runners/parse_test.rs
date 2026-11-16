use super::*;

#[test]
fn valid_header_has_no_explanation() {
    assert_eq!(
        explain_malformed_header(r#" { "Depends": "py-genlayer:x" }"#, 1),
        None
    );
}

#[test]
fn comment_under_header_is_explained() {
    let code = "# { \"Depends\": \"py-genlayer:x\" }\n# my contract\nexit(0)\n";
    let archive = code_to_archive_from_text(bytes::Bytes::from_static(code.as_bytes())).unwrap();
    let header = std::str::from_utf8(archive.data["runner.json"].as_ref()).unwrap();

    let hint = explain_malformed_header(header, 2).unwrap();
    assert!(hint.contains("2 leading comment line(s)"), "{hint}");
    assert!(hint.contains("} my contract"), "{hint}");
    assert!(hint.contains("column"), "{hint}");
    assert!(hint.contains("leave a blank line"), "{hint}");
}

#[test]
fn long_header_is_truncated_on_a_char_boundary() {
    let header = format!(" {}", "\u{e9}".repeat(500));
    let hint = explain_malformed_header(&header, 1).unwrap();
    assert!(hint.contains("..."), "{hint}");
    assert!(hint.len() < 700, "{hint}");
}
