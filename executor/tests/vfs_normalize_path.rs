use genvm::wasi::vfs::split_normalize_path;

fn abs(path: &str) -> String {
    format!("/{}", split_normalize_path(path, true).join("/"))
}

fn rel(path: &str) -> String {
    split_normalize_path(path, false).join("/")
}

// -- Empty input and the root itself ----------------------------------
#[test]
fn empty_input_is_the_root_or_the_empty_path() {
    assert_eq!(abs(""), "/");
    assert_eq!(rel(""), "");
}

#[test]
fn separators_alone_collapse_to_the_root() {
    assert_eq!(abs("/"), "/");
    assert_eq!(abs("//"), "/");
    assert_eq!(abs("///"), "/");
    assert_eq!(rel("/"), "");
}

// -- Separator collapsing ---------------------------------------------
#[test]
fn leading_trailing_and_repeated_separators_are_dropped() {
    assert_eq!(abs("a"), "/a");
    assert_eq!(abs("/a"), "/a");
    assert_eq!(abs("a/"), "/a");
    assert_eq!(abs("/a/b"), "/a/b");
    assert_eq!(abs("a//b"), "/a/b");
    assert_eq!(abs("a///b"), "/a/b");
    assert_eq!(abs("//a//b//"), "/a/b");
    assert_eq!(rel("a//b"), "a/b");
    assert_eq!(rel("/a/b/"), "a/b");
}

// -- `.` components ---------------------------------------------------
#[test]
fn dot_components_are_dropped() {
    assert_eq!(abs("."), "/");
    assert_eq!(abs("./"), "/");
    assert_eq!(abs("./a"), "/a");
    assert_eq!(abs("a/."), "/a");
    assert_eq!(abs("a/./b"), "/a/b");
    assert_eq!(abs("a/././b"), "/a/b");
    assert_eq!(abs("./././"), "/");
    assert_eq!(rel("."), "");
    assert_eq!(rel("a/./b"), "a/b");
}

#[test]
fn names_that_only_look_like_dot_components_are_kept() {
    assert_eq!(abs("..."), "/...");
    assert_eq!(abs("..a"), "/..a");
    assert_eq!(abs("a.."), "/a..");
    assert_eq!(abs(".a"), "/.a");
    assert_eq!(rel("..."), "...");
}

// -- `..` cancellation ------------------------------------------------
#[test]
fn parent_component_cancels_the_preceding_one() {
    assert_eq!(abs("a/.."), "/");
    assert_eq!(abs("a/../b"), "/b");
    assert_eq!(abs("a/b/.."), "/a");
    assert_eq!(abs("a/b/../c"), "/a/c");
    assert_eq!(abs("a/b/c/.."), "/a/b");
    assert_eq!(rel("a/.."), "");
    assert_eq!(rel("a/b/.."), "a");
}

#[test]
fn separators_and_dots_do_not_break_cancellation() {
    assert_eq!(abs("a//../b"), "/b");
    assert_eq!(abs("a/./../b"), "/b");
    assert_eq!(abs("a/.././b"), "/b");
    assert_eq!(abs("a/b/.//../c"), "/a/c");
}

#[test]
fn consecutive_parent_components_cancel_one_component_each() {
    assert_eq!(abs("a/b/../.."), "/");
    assert_eq!(abs("a/b/c/../../d"), "/a/d");
    assert_eq!(abs("a/b/../c/.."), "/a");
    assert_eq!(rel("a/b/../.."), "");
}

// -- `..` with nothing left to cancel ---------------------------------
#[test]
fn absolute_paths_clamp_parent_components_at_the_root() {
    assert_eq!(abs(".."), "/");
    assert_eq!(abs("../.."), "/");
    assert_eq!(abs("/../a"), "/a");
    assert_eq!(abs("../../a"), "/a");
    assert_eq!(abs("a/../../b"), "/b");
    assert_eq!(abs("a/../../../b"), "/b");
}

// -- Relative `..` that cannot be cancelled ---------------------------
#[test]
fn relative_paths_keep_parent_components_they_cannot_cancel() {
    assert_eq!(rel(".."), "..");
    assert_eq!(rel("../.."), "../..");
    assert_eq!(rel("../a"), "../a");
    assert_eq!(rel("a/../.."), "..");
    assert_eq!(rel("a/../../b"), "../b");
    assert_eq!(rel("../a/.."), "..");
}
