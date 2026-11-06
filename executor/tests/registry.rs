use genvm::runners::cache::read_registry;

const HASH: &str = "cnn3rjeozkptmzmzt4ymzznvkqfdedcvpapscmyt6r6cyzjrzsgq";
const OTHER_HASH: &str = "cnn3rjeozkptmzmzt4ymzznvkqfdedcvpapscmyt6r6cyzjrzsga";
const LINE: &str = "genvm/executor/v0.3";
const NEXT_LINE: &str = "genvm/executor/v0.4";

fn fork_names(forks: &genvm::runners::cache::Forks) -> Vec<&str> {
    forks.iter().map(|fork| fork.as_str()).collect()
}

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
                "{OTHER_HASH}": {{"forks": "{LINE}", "supersedes": null}},
                "{HASH}": {{"forks": "{LINE}|{NEXT_LINE}", "supersedes": "{OTHER_HASH}"}}
            }}}}}}"#
        ),
    )
    .unwrap();

    let entry = &got["py-genlayer"];
    assert_eq!(entry.latest.as_str(), HASH);
    let new = &entry.versions[HASH];
    assert_eq!(fork_names(&new.forks), [LINE, NEXT_LINE]);
    assert_eq!(
        new.supersedes.as_ref().map(|s| s.as_str()),
        Some(OTHER_HASH)
    );
    assert_eq!(fork_names(&entry.versions[OTHER_HASH].forks), [LINE]);
}

/// A registry name is joined into `runners_dir` as a path component, so a `..`
/// in it would name an artifact outside the runners tree.
#[test]
fn registry_rejects_runner_name_that_escapes_runners_directory() {
    let got = read(
        "name-traversal",
        &format!(
            r#"{{"../outside": {{"latest": "{HASH}", "versions": {{"{HASH}": {{"forks": "{LINE}"}}}}}}}}"#
        ),
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
            r#"{{"py-genlayer": {{"latest": "{OTHER_HASH}", "versions": {{"{HASH}": {{"forks": "{LINE}"}}}}}}}}"#
        ),
    );

    assert!(got.is_err(), "latest must be one of the listed versions");
}

#[test]
fn registry_rejects_superseding_unlisted_hash() {
    let got = read(
        "supersedes-unlisted",
        &format!(
            r#"{{"py-genlayer": {{"latest": "{HASH}", "versions": {{"{HASH}": {{"forks": "{LINE}", "supersedes": "{OTHER_HASH}"}}}}}}}}"#
        ),
    );

    assert!(got.is_err(), "a superseded runner must still be shipped");
}

#[test]
fn registry_rejects_unknown_version_field() {
    let got = read(
        "unknown-field",
        &format!(
            r#"{{"py-genlayer": {{"latest": "{HASH}", "versions": {{"{HASH}": {{"forks": "{LINE}", "fork": "{LINE}"}}}}}}}}"#
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
fn runner_is_visible_iff_one_of_its_forks_is_active() {
    let got = read(
        "forks",
        &format!(
            r#"{{"py-genlayer": {{"latest": "{HASH}", "versions": {{
                "{HASH}": {{"forks": "{LINE}|{NEXT_LINE}"}}
            }}}}}}"#
        ),
    )
    .unwrap();
    let version = &got["py-genlayer"].versions[HASH];
    let active = |list: &str| genvm::runners::cache::parse_forks(list, ',').unwrap();

    assert!(version.is_visible_in(&active(LINE)));
    assert!(version.is_visible_in(&active(NEXT_LINE)));
    assert!(!version.is_visible_in(&active("genvm/executor/v0.2")));
    assert!(!version.is_visible_in(&genvm::runners::cache::Forks::new()));
}

#[test]
fn registry_rejects_malformed_forks() {
    for forks in [
        "",
        "|",
        "genvm/executor/v0.3|",
        "genvm//v0.3",
        "/genvm",
        "Genvm",
        "genvm executor",
        "genvm,executor",
    ] {
        let got = read(
            "malformed-forks",
            &format!(
                r#"{{"py-genlayer": {{"latest": "{HASH}", "versions": {{"{HASH}": {{"forks": "{forks}"}}}}}}}}"#
            ),
        );
        assert!(got.is_err(), "forks `{forks}` must be rejected");
    }
}

#[test]
fn registry_rejects_missing_forks() {
    let got = read(
        "missing-forks",
        &format!(r#"{{"py-genlayer": {{"latest": "{HASH}", "versions": {{"{HASH}": {{}}}}}}}}"#),
    );

    assert!(got.is_err(), "every version must name its forks");
}
