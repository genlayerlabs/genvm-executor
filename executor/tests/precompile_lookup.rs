//! The precompile cache is read with `Module::deserialize_file`, which trusts the
//! file as native code. Only runners the precompiler actually writes artifacts
//! for, the ones listed in `all.json`, may derive a path into it.

use genvm::runners;

const HASH: &str = "cnn3rjeozkptmzmzt4ymzznvkqfdedcvpapscmyt6r6cyzjrzsgq";

fn unique_dir(tag: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("genvm-{tag}-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

/// A registry holding exactly one builtin runner, `py-genlayer:HASH`.
fn registry_with_one_builtin() -> (std::path::PathBuf, runners::cache::Reader) {
    let root = unique_dir("registry");
    let runners_dir = root.join("runners");
    std::fs::create_dir_all(&runners_dir).unwrap();
    std::fs::write(
        root.join("all.json"),
        format!(r#"{{"py-genlayer": ["{HASH}"]}}"#),
    )
    .unwrap();

    let reader = runners::cache::Reader::new(&runners_dir, &root, false).unwrap();
    (root, reader)
}

#[test]
fn verify_runner_alone_accepts_custom_ids() {
    // This is why the registry lookup exists: `custom:<hash>`, an id an attacker
    // picks via RegisterRunner, is a well-formed `name:hash` pair, so the charset
    // check waves it through. `chain:` and `contract` are excluded only
    // incidentally, by their colon count.
    assert_eq!(
        runners::verify_runner(&format!("custom:{HASH}")),
        Some(("custom", HASH)),
    );
}

#[test]
fn custom_runner_is_not_in_the_registry() {
    let (root, reader) = registry_with_one_builtin();

    assert!(
        reader.has_in_all("py-genlayer", HASH),
        "the builtin the registry lists must be found",
    );
    assert!(
        !reader.has_in_all("custom", HASH),
        "a custom: runner must never resolve a precompiled artifact",
    );
    assert!(
        !reader.has_in_all("py-genlayer", "not-the-listed-hash"),
        "a builtin name with an unlisted hash must not resolve either",
    );

    std::fs::remove_dir_all(&root).unwrap();
}
