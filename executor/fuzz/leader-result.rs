use arbitrary::Arbitrary;
use genvm::{
    calldata, public_abi::VmError, rt::vm::RunOk, wasi::genlayer_sdk::parse_leader_result,
};

#[derive(Debug)]
enum Input {
    Raw(Vec<u8>),
    Framed { code: u8, value: calldata::Value },
    VmErrCode(String),
}

impl<'a> Arbitrary<'a> for Input {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        if u.is_empty() {
            return Ok(Self::Raw(Vec::new()));
        }

        match u.int_in_range(0..=2u8)? {
            0 => Ok(Self::Raw(u.bytes(u.len())?.to_vec())),
            1 => {
                let code = if u.is_empty() { 0 } else { u.arbitrary()? };
                let value = calldata::Value::arbitrary(u).unwrap_or(calldata::Value::Null);
                Ok(Self::Framed { code, value })
            }
            _ => {
                let bytes = u.bytes(u.len())?;
                Ok(Self::VmErrCode(String::from_utf8_lossy(bytes).into_owned()))
            }
        }
    }
}

fn data_from(input: Input) -> Vec<u8> {
    match input {
        Input::Raw(data) => data,
        Input::Framed { code, value } => {
            let mut data = vec![code];
            data.extend_from_slice(&calldata::encode(&value));
            data
        }
        Input::VmErrCode(code) => {
            let mut data = vec![2];
            data.extend_from_slice(code.as_bytes());
            data
        }
    }
}

fn is_derived_namespace(code: &str) -> bool {
    is_code_or_space_extension(code, "absent_leader_nondet_output")
        || is_code_or_space_extension(code, "leader_output")
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
        "absent_leader_nondet_output" | "leader_output malformed"
    ) {
        return true;
    }

    let Some(rest) = code.strip_prefix("leader_output uses_this_error ") else {
        return false;
    };
    rest == "fix_point" || is_gvm32_6(rest)
}

fn assert_parse_properties(data: &[u8]) {
    match parse_leader_result(data) {
        Ok(res) => {
            assert_eq!(
                res.as_bytes(),
                data,
                "accepted leader result must serialize byte-identically"
            );

            match res {
                RunOk::VMError(e, _) => {
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
                RunOk::Return(_) | RunOk::UserError(_) => {
                    assert!(
                        data.len() > 1 && calldata::decode(&data[1..]).is_ok(),
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

fn main() {
    afl::fuzz!(|input: Input| {
        let data = data_from(input);
        assert_parse_properties(&data);
    });
}
