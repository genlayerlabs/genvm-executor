// This file is auto-generated. Do not edit!

#![allow(dead_code, clippy::redundant_static_lifetimes)]

use serde::Serialize;

use std::borrow::Cow;

#[allow(non_snake_case)]
#[rustfmt::skip]
pub mod __VmError {
    use std::borrow::Cow;
    use super::VmError;

    pub struct LeaderOutput;

    impl LeaderOutput {
        pub const fn prefix_(&self) -> &'static str {
            "leader_output"
        }
        pub const fn malformed(&self) -> VmError { VmError(Cow::Borrowed("leader_output malformed")) }
        pub const fn uses_this_error(&self) -> LeaderOutputUsesThisError { LeaderOutputUsesThisError }
        pub const fn extra(&self) -> LeaderOutputExtra { LeaderOutputExtra }
    }

    pub struct LeaderOutputUsesThisError;

    impl LeaderOutputUsesThisError {
        pub fn val_str(&self, v: &str) -> VmError {
            debug_assert!(!v.is_empty(), "leader_output uses_this_error needs a non-empty description");
            VmError(Cow::Owned(format!("leader_output uses_this_error {v}")))
        }
    }

    pub struct LeaderOutputExtra;

    impl LeaderOutputExtra {
        pub fn val_str(&self, v: &str) -> VmError {
            debug_assert!(!v.is_empty(), "leader_output extra needs a non-empty description");
            VmError(Cow::Owned(format!("leader_output extra {v}")))
        }
    }

}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VmError(pub Cow<'static, str>);

impl From<VmError> for String {
    fn from(val: VmError) -> String {
        val.0.into()
    }
}
#[rustfmt::skip]
impl VmError {
    pub const fn malformed_entry() -> Self { Self(Cow::Borrowed("malformed_entry")) }
    pub const fn leader_output() -> __VmError::LeaderOutput { __VmError::LeaderOutput }
}

#[rustfmt::skip]
impl VmError {
    /// Whether `s` is a well-formed `vm_error` path.
    pub fn is_valid_(s: &str) -> bool {
        if matches!(s,
            "malformed_entry" |
            "leader_output malformed"
        ) {
            return true;
        }
        if let Some(rest) = s.strip_prefix("leader_output uses_this_error ") {
            if !rest.is_empty() {
                return true;
            }
        }
        if let Some(rest) = s.strip_prefix("leader_output extra ") {
            if !rest.is_empty() {
                return true;
            }
        }
        false
    }
}

// EOF
