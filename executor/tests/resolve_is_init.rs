use genvm::resolve_is_init;

#[test]
fn absent_is_init_follows_code() {
    assert!(resolve_is_init(None, Some(&[1])).unwrap());
    assert!(!resolve_is_init(None, None).unwrap());
}

#[test]
fn matching_is_init_is_accepted() {
    assert!(resolve_is_init(Some(true), Some(&[1])).unwrap());
    assert!(!resolve_is_init(Some(false), None).unwrap());
}

#[test]
fn code_without_is_init_is_a_host_error() {
    let err = resolve_is_init(Some(false), Some(&[1])).unwrap_err();
    assert!(
        err.to_string().contains("is_init"),
        "unexpected error: {err}"
    );
}

#[test]
fn is_init_without_code_is_a_host_error() {
    let err = resolve_is_init(Some(true), None).unwrap_err();
    assert!(
        err.to_string().contains("is_init"),
        "unexpected error: {err}"
    );
}
