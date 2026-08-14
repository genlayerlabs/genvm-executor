use std::collections::BTreeMap;

use genvm::wasi::{preview1::join_dir_and_path, vfs::Trie};

/// A preopen granting `path` and everything beneath it. `granted(&[])` grants the
/// whole filesystem, which is what a contract normally runs with.
fn granted(path: &[&str]) -> Trie<()> {
    let mut node = Trie::Leaf(());
    for component in path.iter().rev() {
        node = Trie::Dir(BTreeMap::from([((*component).to_owned(), node)]));
    }
    node
}

// -- Component algebra, with the whole filesystem granted -------------
#[test]
fn relative_path_is_appended_to_the_directory() {
    let joined = join_dir_and_path(&granted(&[]), &["a"], "b/c").unwrap();

    assert_eq!(joined, ["a", "b", "c"]);
}

#[test]
fn dot_and_empty_components_are_dropped() {
    let joined = join_dir_and_path(&granted(&[]), &["a"], "./b//c/.").unwrap();

    assert_eq!(joined, ["a", "b", "c"]);
}

/// The sandbox boundary is the preopen, not an individual directory fd, so a
/// `..` that stays under the preopen root must resolve rather than be refused.
#[test]
fn parent_component_resolves_within_the_preopen() {
    let joined = join_dir_and_path(&granted(&[]), &["parent", "child"], "..")
        .expect("`..` that stays inside the preopen must resolve its parent");

    assert_eq!(joined, ["parent"]);
}

#[test]
fn parent_components_clamp_at_the_root() {
    let joined = join_dir_and_path(&granted(&[]), &["a"], "../../../b").unwrap();

    assert_eq!(joined, ["b"]);
}

#[test]
fn the_root_itself_is_reachable_when_granted() {
    let joined = join_dir_and_path(&granted(&[]), &[] as &[&str], "").unwrap();

    assert_eq!(joined, [] as [&str; 0]);
}

// -- A preopen granting only a subtree --------------------------------
#[test]
fn a_path_under_the_grant_is_allowed() {
    let preopen = granted(&["allowed"]);

    assert_eq!(
        join_dir_and_path(&preopen, &["allowed"], "file").unwrap(),
        ["allowed", "file"]
    );
    assert_eq!(
        join_dir_and_path(&preopen, &["allowed", "deep"], "file").unwrap(),
        ["allowed", "deep", "file"]
    );
}

#[test]
fn the_granted_directory_itself_is_allowed() {
    let joined = join_dir_and_path(&granted(&["allowed"]), &["allowed"], "").unwrap();

    assert_eq!(joined, ["allowed"]);
}

#[test]
fn a_sibling_of_the_grant_is_refused() {
    assert!(
        join_dir_and_path(&granted(&["allowed"]), &[] as &[&str], "other/file").is_err(),
        "a path outside the granted subtree must not resolve"
    );
}

/// An ungranted ancestor of the grant is itself off limits: holding `/allowed`
/// does not confer the right to enumerate `/`.
#[test]
fn an_ancestor_of_the_grant_is_refused() {
    assert!(
        join_dir_and_path(&granted(&["allowed"]), &[] as &[&str], "").is_err(),
        "the root must not resolve when only a subtree is granted"
    );
}

/// The boundary check happens after normalization, so `..` cannot walk out of
/// the grant by cancelling components that were never checked on their own.
#[test]
fn parent_components_cannot_escape_the_grant() {
    let preopen = granted(&["allowed"]);

    assert!(
        join_dir_and_path(&preopen, &["allowed", "sub"], "../../other").is_err(),
        "`..` must not reach a sibling of the granted subtree"
    );
    assert!(
        join_dir_and_path(&preopen, &["allowed"], "..").is_err(),
        "`..` must not reach the ungranted parent of the grant"
    );
}

#[test]
fn parent_components_inside_the_grant_still_resolve() {
    let joined = join_dir_and_path(&granted(&["allowed"]), &["allowed", "sub"], "..")
        .expect("`..` staying inside the grant must resolve");

    assert_eq!(joined, ["allowed"]);
}

// -- A grant several components deep ----------------------------------
#[test]
fn a_multi_component_grant_covers_only_its_own_subtree() {
    let preopen = granted(&["deep", "nested"]);

    assert_eq!(
        join_dir_and_path(&preopen, &["deep", "nested"], "file").unwrap(),
        ["deep", "nested", "file"]
    );
    assert_eq!(
        join_dir_and_path(&preopen, &[] as &[&str], "deep/nested").unwrap(),
        ["deep", "nested"]
    );
}

/// Every component on the way down to the grant is a `Dir` in the preopen trie,
/// and a `Dir` carries no permission of its own.
#[test]
fn an_intermediate_component_of_the_grant_is_refused() {
    let preopen = granted(&["deep", "nested"]);

    assert!(
        join_dir_and_path(&preopen, &[] as &[&str], "deep").is_err(),
        "the intermediate `/deep` must not resolve"
    );
    assert!(
        join_dir_and_path(&preopen, &["deep"], "sibling").is_err(),
        "a sibling of the grant under `/deep` must not resolve"
    );
}

#[test]
fn parent_components_cannot_escape_a_multi_component_grant() {
    let preopen = granted(&["deep", "nested"]);

    assert!(
        join_dir_and_path(&preopen, &["deep", "nested"], "../sibling").is_err(),
        "`..` must not step out of the grant into its parent directory"
    );
    assert_eq!(
        join_dir_and_path(&preopen, &["deep", "nested", "sub"], "..").unwrap(),
        ["deep", "nested"],
        "`..` back onto the grant itself must still resolve"
    );
}

// -- Several disjoint grants ------------------------------------------
fn two_grants() -> Trie<()> {
    Trie::Dir(BTreeMap::from([
        ("first".to_owned(), Trie::Leaf(())),
        ("second".to_owned(), Trie::Leaf(())),
    ]))
}

#[test]
fn each_grant_resolves_independently() {
    let preopen = two_grants();

    assert_eq!(
        join_dir_and_path(&preopen, &["first"], "file").unwrap(),
        ["first", "file"]
    );
    assert_eq!(
        join_dir_and_path(&preopen, &["second"], "file").unwrap(),
        ["second", "file"]
    );
}

#[test]
fn an_ungranted_sibling_of_the_grants_is_refused() {
    assert!(
        join_dir_and_path(&two_grants(), &[] as &[&str], "third").is_err(),
        "an ungranted sibling must not resolve"
    );
}
