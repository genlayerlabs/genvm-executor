use std::collections::BTreeMap;

use genvm::runners::{Archive, ArchiveCache, InitAction, WasmMode};

fn archive_with_runner_json(contents: &str) -> ArchiveCache {
    let contents = bytes::Bytes::copy_from_slice(contents.as_bytes());
    let files = Archive {
        total_size: contents.len() as u32,
        data: BTreeMap::from([("runner.json".to_owned(), contents)]),
    };
    ArchiveCache::new(symbol_table::GlobalSymbol::from("runner:test"), files)
}

fn assert_malformed_runner(actual: &genvm::rt::errors::Error) {
    let expected = genvm::public_abi::VmError::invalid_contract()
        .runner()
        .malformed();
    assert!(
        matches!(&actual.kind, genvm::rt::errors::ErrorKind::Vm(code) if code == &expected),
        "unexpected error kind: {:?}",
        actual.kind
    );
}

fn when_mode(contents: &str) -> Result<WasmMode, String> {
    let action: InitAction = serde_json::from_str(contents).map_err(|error| error.to_string())?;
    match action {
        InitAction::When { cond, .. } => Ok(cond),
        other => Err(format!("expected When action, got {other:?}")),
    }
}

#[tokio::test]
async fn stray_action_field_is_a_malformed_runner_error() {
    let archive =
        archive_with_runner_json(r#"{"MapFile":{"file":"a","to":"/b","destination":"/c"}}"#);

    let error = archive
        .get_actions()
        .await
        .expect_err("stray action field was accepted");

    assert_malformed_runner(&error);
    assert!(
        error.to_string().contains("destination"),
        "error does not name the stray field: {error}"
    );
}

#[tokio::test]
async fn top_level_schema_annotation_is_accepted() {
    let archive = archive_with_runner_json(
        r#"{"StartWasm":"main.wasm","$schema":"https://example.com/runner.schema.json"}"#,
    );

    let actual = archive.get_actions().await;
    assert!(actual.is_ok(), "schema annotation was rejected: {actual:?}");
}

#[tokio::test]
async fn duplicate_action_payload_field_is_rejected() {
    let archive =
        archive_with_runner_json(r#"{"MapFile":{"file":"a","file":"b","to":"/contract.py"}}"#);

    let error = archive
        .get_actions()
        .await
        .expect_err("duplicate action field was accepted");

    assert_malformed_runner(&error);
    assert!(
        error.to_string().contains("duplicate field `file`"),
        "unexpected duplicate-field error: {error}"
    );
}

#[test]
fn bang_det_selects_nondeterministic_wasm() {
    let actual = when_mode(r#"{"When":{"cond":"!det","action":{"StartWasm":"main.wasm"}}}"#);
    assert!(
        matches!(actual, Ok(WasmMode::Nondet)),
        "unexpected parse result: {actual:?}"
    );
}

#[test]
fn old_nondet_spelling_is_rejected() {
    let actual = when_mode(r#"{"When":{"cond":"nondet","action":{"StartWasm":"main.wasm"}}}"#);
    assert!(actual.is_err(), "old spelling was accepted: {actual:?}");
}

#[tokio::test]
async fn duplicate_schema_annotation_is_rejected() {
    let archive = archive_with_runner_json(
        r#"{"$schema":"https://a.example/s.json","$schema":"https://b.example/s.json","StartWasm":"main.wasm"}"#,
    );

    let error = archive
        .get_actions()
        .await
        .expect_err("duplicate $schema was accepted");

    assert_malformed_runner(&error);
    assert!(
        error.to_string().contains("$schema"),
        "error does not name the field: {error}"
    );
}

/// A runner.json carries exactly one action. The top level strips `$schema` and
/// then hands the rest to the same derived impl nested actions use, so a second
/// action must not slip through that seam.
#[tokio::test]
async fn a_second_top_level_action_is_rejected() {
    let archive = archive_with_runner_json(r#"{"StartWasm":"main.wasm","LinkWasm":"other.wasm"}"#);

    let error = archive
        .get_actions()
        .await
        .expect_err("two top-level actions were accepted");

    assert_malformed_runner(&error);
    assert!(
        error.to_string().contains("LinkWasm"),
        "error does not name the extra action: {error}"
    );
}

/// `$schema` is a top-level annotation only; nested actions go through the
/// derived impl, whose `deny_unknown_fields` must reject it.
#[tokio::test]
async fn nested_schema_annotation_is_rejected() {
    let archive = archive_with_runner_json(
        r#"{"Seq":[{"$schema":"https://a.example/s.json","StartWasm":"main.wasm"}]}"#,
    );

    let error = archive
        .get_actions()
        .await
        .expect_err("nested $schema was accepted");

    assert_malformed_runner(&error);
}
