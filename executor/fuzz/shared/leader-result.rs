use genvm::{
    public_abi::VmError, rt::vm::ContractOutcome, wasi::genlayer_sdk::parse_leader_result,
};

fn is_derived_namespace(code: &str) -> bool {
    is_code_or_space_extension(code, "leader_fault nondet_output")
}

fn is_code_or_space_extension(code: &str, prefix: &str) -> bool {
    code == prefix
        || code
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with(' '))
}

fn is_gvm32_6(value: &str) -> bool {
    value.len() == 6
        && value
            .bytes()
            .all(|b| b"0123456789abcdefghjkmnpqrstvwxyz".contains(&b))
}

fn is_closed_derived_error(code: &str) -> bool {
    if matches!(
        code,
        "leader_fault nondet_output absent" | "leader_fault nondet_output malformed"
    ) {
        return true;
    }

    let Some(rest) = code.strip_prefix("leader_fault nondet_output uses_this_error ") else {
        return false;
    };
    rest == "fix_point" || is_gvm32_6(rest)
}

pub fn assert_parse_properties(data: &[u8]) {
    match parse_leader_result(data) {
        Ok(res) => {
            assert_eq!(
                res.encode().into_bytes().as_ref(),
                data,
                "accepted leader result must serialize byte-identically"
            );

            match res {
                ContractOutcome::VMError(e, _) => {
                    assert!(
                        VmError::is_valid_(&e.0),
                        "accepted vm_error must be a valid public ABI code: {:?}",
                        e.0
                    );
                    assert!(
                        !e.0.contains(" # "),
                        "accepted vm_error must not carry detail: {:?}",
                        e.0
                    );
                    assert!(
                        !is_derived_namespace(&e.0),
                        "accepted vm_error must not be in the derived namespace: {:?}",
                        e.0
                    );
                }
                ContractOutcome::Return(_) | ContractOutcome::UserError(_) => {
                    assert!(
                        data.len() > 1 && genvm::calldata::decode(&data[1..]).is_ok(),
                        "validate-only and materializing calldata decode must agree"
                    );
                }
            }
        }
        Err(e) => {
            assert!(
                is_closed_derived_error(&e.0),
                "derived parse error must match the closed grammar: {:?}",
                e.0
            );

            let mut reproposal = vec![2];
            reproposal.extend_from_slice(e.0.as_bytes());
            assert!(
                parse_leader_result(&reproposal).is_err(),
                "derived leader result must not be proposable verbatim: {:?}",
                e.0
            );
        }
    }
}
