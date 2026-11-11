use super::*;

#[test]
fn add_env_bounds_expansion_before_visiting_later_variables() {
    let limiter = rt::memlimiter::Limiter::new();
    let mut env = Env::new(limiter.clone());
    let value = "x".repeat(runner_limits::ENV_VALUE_LEN.into_int_comptime());
    env.set_patching("", &value).unwrap();
    let remaining = limiter.get_remaining_memory();

    for template in ["${}${}", "${}${}${missing}"] {
        let error = env.set_patching("result", template).unwrap_err();
        assert!(error.to_string().contains("limit"), "{error}");
        assert!(matches!(error.kind, rt::errors::ErrorKind::Vm(code)
            if code == abi::consts::VmError::invalid_contract().runner().malformed()));
        assert!(!env.vars.contains_key("result"));
        assert_eq!(limiter.get_remaining_memory(), remaining);
    }
    env.set_patching("result", "${}").unwrap();
    assert_eq!(env.vars["result"], value);
}

#[test]
fn add_env_expansion_errors_identify_the_variable() {
    let mut env = Env::new(rt::memlimiter::Limiter::new());
    let limit: usize = runner_limits::ENV_VALUE_LEN.into_int_comptime();
    for template in ["x".repeat(limit + 1), "${missing}".to_owned()] {
        let error = env.set_patching("MY_ENV", &template).unwrap_err();
        assert!(error.to_string().contains("MY_ENV"), "{error}");
    }
}

#[test]
fn add_env_preserves_literal_limit_unknown_and_nul_checks() {
    let mut env = Env::new(rt::memlimiter::Limiter::new());
    let limit: usize = runner_limits::ENV_VALUE_LEN.into_int_comptime();
    for template in [
        "x".repeat(limit + 1),
        "${missing}".to_owned(),
        "\0".to_owned(),
    ] {
        let error = env.set_patching("result", &template).unwrap_err();
        assert!(matches!(error.kind, rt::errors::ErrorKind::Vm(code)
            if code == abi::consts::VmError::invalid_contract().runner().malformed()));
        assert!(!env.vars.contains_key("result"));
    }
}
