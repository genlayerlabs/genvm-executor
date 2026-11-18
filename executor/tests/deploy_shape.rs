use genvm::check_deploy_shape;

#[test]
fn deploy_accepts_code_or_none() {
    check_deploy_shape(true, Some(&[1])).unwrap();
    check_deploy_shape(true, None).unwrap();
}

#[test]
fn call_without_code_passes() {
    check_deploy_shape(false, None).unwrap();
}

#[test]
fn call_with_code_is_a_host_error() {
    let err = check_deploy_shape(false, Some(&[1])).unwrap_err();
    assert!(
        err.to_string().contains("is_init"),
        "unexpected error: {err}"
    );
}
