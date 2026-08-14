use genvm::runners::cache::read_registry;

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

/// A registry name is joined into `runners_dir` as a path component, so a `..`
/// in it would name an artifact outside the runners tree.
#[test]
fn registry_rejects_runner_name_that_escapes_runners_directory() {
    let hash = "0".repeat(32);
    let path = write_registry(
        "name-traversal",
        &format!(r#"{{"../outside": ["{hash}"]}}"#),
    );

    let got = read_registry::<String, Vec<String>>(&path);
    std::fs::remove_file(&path).unwrap();

    assert!(
        got.is_err(),
        "`../outside` must not be accepted as a runner name"
    );
}
