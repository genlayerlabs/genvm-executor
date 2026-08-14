use genvm::runners;

/// `args_get` hands the guest a NUL-terminated buffer, so an embedded NUL would
/// truncate the argument the guest sees.
#[test]
fn set_args_rejects_embedded_nul() {
    let args = vec!["visible\0hidden".to_owned()];

    runners::actions::SetArgs(args).validate().unwrap_err();
}

/// `environ_get` frames entries as `name=value\0`, so a `=` in a name or a NUL
/// anywhere reframes what the guest parses.
#[test]
fn set_env_rejects_entries_breaking_framing() {
    let invalid = [
        ("BAD=NAME", "value"),
        ("BAD\0NAME", "value"),
        ("GOOD", "visible\0hidden"),
    ];

    let mut env = runners::actions::Env::new(genvm::rt::memlimiter::Limiter::new());

    for (name, value) in &invalid {
        let res = env.set_patching(name, value);
        assert!(res.is_err(), "unexpected error for {name:?}={value:?}");
    }
}
