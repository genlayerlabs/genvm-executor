use genvm::runners::cache::read_registry;

const HASH: &str = "cnn3rjeozkptmzmzt4ymzznvkqfdedcvpapscmyt6r6cyzjrzsgq";
const OTHER_HASH: &str = "cnn3rjeozkptmzmzt4ymzznvkqfdedcvpapscmyt6r6cyzjrzsga";

fn write_registry(label: &str, contents: &str) -> std::path::PathBuf {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time before epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "genvm-registry-{label}-{}-{suffix}.json",
        std::process::id()
    ));
    std::fs::write(&path, contents).unwrap();
    path
}

fn read(label: &str, contents: &str) -> anyhow::Result<genvm::runners::cache::Registry> {
    let path = write_registry(label, contents);
    let got = read_registry(&path);
    std::fs::remove_file(&path).unwrap();
    got
}

#[test]
fn registry_reads_entry_with_all_fields() {
    let got = read(
        "full",
        &format!(
            r#"{{"py-genlayer": {{"latest": "{HASH}", "versions": {{
                "{OTHER_HASH}": {{"available_after": null, "supersedes": null}},
                "{HASH}": {{"available_after": "2026-12-01T00:00:00Z", "supersedes": "{OTHER_HASH}"}}
            }}}}}}"#
        ),
    )
    .unwrap();

    let entry = &got["py-genlayer"];
    assert_eq!(entry.latest.as_str(), HASH);
    let new = &entry.versions[HASH];
    assert_eq!(
        new.available_after,
        Some("2026-12-01T00:00:00Z".parse().unwrap())
    );
    assert_eq!(
        new.supersedes.as_ref().map(|s| s.as_str()),
        Some(OTHER_HASH)
    );
    assert_eq!(entry.versions[OTHER_HASH].available_after, None);
}

/// A registry name is joined into `runners_dir` as a path component, so a `..`
/// in it would name an artifact outside the runners tree.
#[test]
fn registry_rejects_runner_name_that_escapes_runners_directory() {
    let got = read(
        "name-traversal",
        &format!(r#"{{"../outside": {{"latest": "{HASH}", "versions": {{"{HASH}": {{}}}}}}}}"#),
    );

    assert!(
        got.is_err(),
        "`../outside` must not be accepted as a runner name"
    );
}

#[test]
fn registry_rejects_latest_outside_versions() {
    let got = read(
        "latest-unlisted",
        &format!(
            r#"{{"py-genlayer": {{"latest": "{OTHER_HASH}", "versions": {{"{HASH}": {{}}}}}}}}"#
        ),
    );

    assert!(got.is_err(), "latest must be one of the listed versions");
}

#[test]
fn registry_rejects_superseding_unlisted_hash() {
    let got = read(
        "supersedes-unlisted",
        &format!(
            r#"{{"py-genlayer": {{"latest": "{HASH}", "versions": {{"{HASH}": {{"supersedes": "{OTHER_HASH}"}}}}}}}}"#
        ),
    );

    assert!(got.is_err(), "a superseded runner must still be shipped");
}

#[test]
fn registry_rejects_unknown_version_field() {
    let got = read(
        "unknown-field",
        &format!(
            r#"{{"py-genlayer": {{"latest": "{HASH}", "versions": {{"{HASH}": {{"availabe_after": null}}}}}}}}"#
        ),
    );

    assert!(
        got.is_err(),
        "a misspelled field must not be silently dropped"
    );
}

#[test]
fn registry_rejects_previous_list_format() {
    let got = read("list-format", &format!(r#"{{"py-genlayer": ["{HASH}"]}}"#));

    assert!(
        got.is_err(),
        "the pre-versions list format must be rejected"
    );
}

#[test]
fn runner_becomes_available_at_its_available_after() {
    let got = read(
        "available-after",
        &format!(
            r#"{{"py-genlayer": {{"latest": "{HASH}", "versions": {{
                "{HASH}": {{"available_after": "2026-12-01T00:00:00Z"}},
                "{OTHER_HASH}": {{}}
            }}}}}}"#
        ),
    )
    .unwrap();
    let versions = &got["py-genlayer"].versions;
    let at = |s: &str| s.parse::<chrono::DateTime<chrono::Utc>>().unwrap();

    let gated = &versions[HASH];
    assert!(!gated.is_available_at(at("2026-11-30T23:59:59Z")));
    assert!(gated.is_available_at(at("2026-12-01T00:00:00Z")));
    assert!(gated.is_available_at(at("2027-01-01T00:00:00Z")));

    let ungated = &versions[OTHER_HASH];
    assert!(ungated.is_available_at(chrono::DateTime::UNIX_EPOCH));
}
