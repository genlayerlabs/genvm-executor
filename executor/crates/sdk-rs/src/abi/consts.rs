// This file is auto-generated. Do not edit!

#![allow(dead_code, clippy::redundant_static_lifetimes)]

use serde::{Deserialize, Serialize};

use std::borrow::Cow;

#[derive(
    Debug,
    PartialEq,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    ::genlayer_calldata::Encode,
    ::genlayer_calldata::Decode,
)]
#[repr(u8)]
pub enum ResultCode {
    Return = 0,
    UserError = 1,
    VmError = 2,
    InternalError = 3,
}

impl ResultCode {
    pub const SIZE: usize = 4;
    pub fn value(self) -> u8 {
        match self {
            ResultCode::Return => 0,
            ResultCode::UserError => 1,
            ResultCode::VmError => 2,
            ResultCode::InternalError => 3,
        }
    }
    pub fn str_snake_case(self) -> &'static str {
        match self {
            ResultCode::Return => "return",
            ResultCode::UserError => "user_error",
            ResultCode::VmError => "vm_error",
            ResultCode::InternalError => "internal_error",
        }
    }
}

impl TryFrom<u8> for ResultCode {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, ()> {
        match value {
            0 => Ok(ResultCode::Return),
            1 => Ok(ResultCode::UserError),
            2 => Ok(ResultCode::VmError),
            3 => Ok(ResultCode::InternalError),
            _ => Err(()),
        }
    }
}
#[derive(
    Debug,
    PartialEq,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    ::genlayer_calldata::Encode,
    ::genlayer_calldata::Decode,
)]
#[repr(u8)]
pub enum StorageType {
    Default = 0,
    LatestFinal = 1,
    LatestNonFinal = 2,
}

impl StorageType {
    pub const SIZE: usize = 3;
    pub fn value(self) -> u8 {
        match self {
            StorageType::Default => 0,
            StorageType::LatestFinal => 1,
            StorageType::LatestNonFinal => 2,
        }
    }
    pub fn str_snake_case(self) -> &'static str {
        match self {
            StorageType::Default => "default",
            StorageType::LatestFinal => "latest_final",
            StorageType::LatestNonFinal => "latest_non_final",
        }
    }
}

impl TryFrom<u8> for StorageType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, ()> {
        match value {
            0 => Ok(StorageType::Default),
            1 => Ok(StorageType::LatestFinal),
            2 => Ok(StorageType::LatestNonFinal),
            _ => Err(()),
        }
    }
}
#[derive(
    Debug,
    PartialEq,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    ::genlayer_calldata::Encode,
    ::genlayer_calldata::Decode,
)]
#[repr(u8)]
pub enum EntryKind {
    Main = 0,
    Sandbox = 1,
    ConsensusStage = 2,
}

impl EntryKind {
    pub const SIZE: usize = 3;
    pub fn value(self) -> u8 {
        match self {
            EntryKind::Main => 0,
            EntryKind::Sandbox => 1,
            EntryKind::ConsensusStage => 2,
        }
    }
    pub fn str_snake_case(self) -> &'static str {
        match self {
            EntryKind::Main => "main",
            EntryKind::Sandbox => "sandbox",
            EntryKind::ConsensusStage => "consensus_stage",
        }
    }
}

impl TryFrom<u8> for EntryKind {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, ()> {
        match value {
            0 => Ok(EntryKind::Main),
            1 => Ok(EntryKind::Sandbox),
            2 => Ok(EntryKind::ConsensusStage),
            _ => Err(()),
        }
    }
}
#[derive(
    Debug,
    PartialEq,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    ::genlayer_calldata::Encode,
    ::genlayer_calldata::Decode,
)]
#[repr(u32)]
pub enum Permissions {
    CanUseBalanceForMessageFees = 1,
}

impl Permissions {
    pub fn value(self) -> u32 {
        match self {
            Permissions::CanUseBalanceForMessageFees => 1,
        }
    }
    pub fn str_snake_case(self) -> &'static str {
        match self {
            Permissions::CanUseBalanceForMessageFees => "can_use_balance_for_message_fees",
        }
    }
}

impl TryFrom<u32> for Permissions {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, ()> {
        match value {
            1 => Ok(Permissions::CanUseBalanceForMessageFees),
            _ => Err(()),
        }
    }
}
pub mod memory_limiter_consts {
    pub const TABLE_ENTRY: u32 = 64;
    pub const FILE_MAPPING: u32 = 256;
    pub const FD_ALLOCATION: u32 = 96;
    pub const RUNNER_LOAD_COST: u32 = 4096;
    pub const VM_SPAWN_COST: u32 = 134217728;
}

pub mod root_offsets {
    pub const MAJOR: u32 = 0;
    pub const CONTRACT: u32 = 1;
    pub const CODE: u32 = 2;
    pub const LOCKED_SLOTS: u32 = 3;
    pub const UPGRADERS: u32 = 4;
    pub const CODE_SLOT: u32 = 5;
    pub const PERMISSIONS: u32 = 37;
}

pub mod top_limits {
    pub const NONDET_BLOCKS: u32 = 4096;
    pub const LOCKED_SLOTS: u32 = 256;
    pub const UPGRADERS: u32 = 32;
    pub const VM_RECURSION: u32 = 512;
    pub const WEB_REQUEST_MIN_SPACE: u32 = 65536;
    pub const WEB_RENDER_MIN_SPACE: u32 = 134217728;
    pub const MAX_FDS: u32 = 1024;
    pub const WASM_CALL_DEPTH: u32 = 1024;
    pub const WASM_STACK_VALUE_SLOTS: u32 = 65535;
}

#[derive(
    Debug,
    PartialEq,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    ::genlayer_calldata::Encode,
    ::genlayer_calldata::Decode,
)]
pub enum SpecialMethod {
    GetSchema,
    ErroredMessage,
}

impl SpecialMethod {
    pub fn value(self) -> &'static str {
        match self {
            SpecialMethod::GetSchema => "#get-schema",
            SpecialMethod::ErroredMessage => "#error",
        }
    }
    pub fn str_snake_case(self) -> &'static str {
        match self {
            SpecialMethod::GetSchema => "get_schema",
            SpecialMethod::ErroredMessage => "errored_message",
        }
    }
}

impl TryFrom<&str> for SpecialMethod {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, ()> {
        match value {
            "#get-schema" => Ok(SpecialMethod::GetSchema),
            "#error" => Ok(SpecialMethod::ErroredMessage),
            _ => Err(()),
        }
    }
}
#[allow(non_snake_case)]
#[rustfmt::skip]
pub mod __VmError {
    use std::borrow::Cow;
    use super::VmError;

    pub struct WasmTrap;

    impl WasmTrap {
        pub const fn val(&self) -> VmError { VmError(Cow::Borrowed("wasm_trap")) }
        pub const fn prefix_(&self) -> &'static str {
            "wasm_trap"
        }
        pub const fn unreachable(&self) -> VmError { VmError(Cow::Borrowed("wasm_trap unreachable")) }
        pub const fn stack_overflow(&self) -> VmError { VmError(Cow::Borrowed("wasm_trap stack_overflow")) }
        pub const fn memory_out_of_bounds(&self) -> VmError { VmError(Cow::Borrowed("wasm_trap memory_out_of_bounds")) }
        pub const fn table_out_of_bounds(&self) -> VmError { VmError(Cow::Borrowed("wasm_trap table_out_of_bounds")) }
        pub const fn indirect_call_to_null(&self) -> VmError { VmError(Cow::Borrowed("wasm_trap indirect_call_to_null")) }
        pub const fn bad_signature(&self) -> VmError { VmError(Cow::Borrowed("wasm_trap bad_signature")) }
        pub const fn integer_overflow(&self) -> VmError { VmError(Cow::Borrowed("wasm_trap integer_overflow")) }
        pub const fn integer_divide_by_zero(&self) -> VmError { VmError(Cow::Borrowed("wasm_trap integer_divide_by_zero")) }
        pub const fn bad_conversion_to_integer(&self) -> VmError { VmError(Cow::Borrowed("wasm_trap bad_conversion_to_integer")) }
        pub const fn heap_misaligned(&self) -> VmError { VmError(Cow::Borrowed("wasm_trap heap_misaligned")) }
        pub const fn atomic_wait_non_shared_memory(&self) -> VmError { VmError(Cow::Borrowed("wasm_trap atomic_wait_non_shared_memory")) }
        pub const fn out_of_fuel(&self) -> VmError { VmError(Cow::Borrowed("wasm_trap out_of_fuel")) }
        pub const fn interrupt(&self) -> VmError { VmError(Cow::Borrowed("wasm_trap interrupt")) }
        pub const fn nondet_instruction(&self) -> VmError { VmError(Cow::Borrowed("wasm_trap nondet_instruction")) }
        pub const fn fault(&self) -> VmError { VmError(Cow::Borrowed("wasm_trap fault")) }
    }

    pub struct OutOfMemory;

    impl OutOfMemory {
        pub const fn val(&self) -> VmError { VmError(Cow::Borrowed("out_of memory")) }
        pub const fn prefix_(&self) -> &'static str {
            "out_of memory"
        }
        pub const fn wasm_memory(&self) -> VmError { VmError(Cow::Borrowed("out_of memory wasm_memory")) }
        pub const fn wasm_table(&self) -> VmError { VmError(Cow::Borrowed("out_of memory wasm_table")) }
    }

    pub struct OutOfReceipt;

    impl OutOfReceipt {
        pub const fn prefix_(&self) -> &'static str {
            "out_of receipt"
        }
        pub const fn nondet_output(&self) -> VmError { VmError(Cow::Borrowed("out_of receipt nondet_output")) }
        pub const fn message(&self) -> VmError { VmError(Cow::Borrowed("out_of receipt message")) }
        pub const fn event(&self) -> VmError { VmError(Cow::Borrowed("out_of receipt event")) }
    }

    pub struct OutOfMessageFee;

    impl OutOfMessageFee {
        pub const fn prefix_(&self) -> &'static str {
            "out_of message_fee"
        }
        pub const fn total(&self) -> VmError { VmError(Cow::Borrowed("out_of message_fee total")) }
        pub const fn node(&self) -> VmError { VmError(Cow::Borrowed("out_of message_fee node")) }
    }

    pub struct OutOf;

    impl OutOf {
        pub const fn prefix_(&self) -> &'static str {
            "out_of"
        }
        pub const fn storage(&self) -> VmError { VmError(Cow::Borrowed("out_of storage")) }
        pub const fn vm_recursion(&self) -> VmError { VmError(Cow::Borrowed("out_of vm_recursion")) }
        pub const fn nondet_blocks(&self) -> VmError { VmError(Cow::Borrowed("out_of nondet_blocks")) }
        pub const fn locked_slots(&self) -> VmError { VmError(Cow::Borrowed("out_of locked_slots")) }
        pub const fn upgraders(&self) -> VmError { VmError(Cow::Borrowed("out_of upgraders")) }
        pub const fn fds(&self) -> VmError { VmError(Cow::Borrowed("out_of fds")) }
        pub const fn memory(&self) -> OutOfMemory { OutOfMemory }
        pub const fn receipt(&self) -> OutOfReceipt { OutOfReceipt }
        pub const fn message_fee(&self) -> OutOfMessageFee { OutOfMessageFee }
    }

    pub struct Fee;

    impl Fee {
        pub const fn prefix_(&self) -> &'static str {
            "fee"
        }
        pub const fn no_matching_node(&self) -> VmError { VmError(Cow::Borrowed("fee no_matching_node")) }
        pub const fn below_minimum(&self) -> VmError { VmError(Cow::Borrowed("fee below_minimum")) }
        pub const fn too_many_rounds(&self) -> VmError { VmError(Cow::Borrowed("fee too_many_rounds")) }
    }

    pub struct Evm;

    impl Evm {
        pub const fn prefix_(&self) -> &'static str {
            "evm"
        }
        pub const fn reverted(&self) -> VmError { VmError(Cow::Borrowed("evm reverted")) }
    }

    pub struct InvalidContractWasm;

    impl InvalidContractWasm {
        pub const fn prefix_(&self) -> &'static str {
            "invalid_contract wasm"
        }
        pub const fn validating(&self) -> VmError { VmError(Cow::Borrowed("invalid_contract wasm validating")) }
        pub const fn linking(&self) -> VmError { VmError(Cow::Borrowed("invalid_contract wasm linking")) }
        pub const fn entrypoint(&self) -> VmError { VmError(Cow::Borrowed("invalid_contract wasm entrypoint")) }
    }

    pub struct InvalidContract;

    impl InvalidContract {
        pub const fn val(&self) -> VmError { VmError(Cow::Borrowed("invalid_contract")) }
        pub const fn prefix_(&self) -> &'static str {
            "invalid_contract"
        }
        pub const fn absent_runner_comment(&self) -> VmError { VmError(Cow::Borrowed("invalid_contract absent_runner_comment")) }
        pub const fn not_utf8_text(&self) -> VmError { VmError(Cow::Borrowed("invalid_contract not_utf8_text")) }
        pub const fn malformed_runner(&self) -> VmError { VmError(Cow::Borrowed("invalid_contract malformed_runner")) }
        pub const fn major_mismatch(&self) -> VmError { VmError(Cow::Borrowed("invalid_contract major_mismatch")) }
        pub const fn wasm(&self) -> InvalidContractWasm { InvalidContractWasm }
    }

    pub struct ExitCode;

    impl ExitCode {
        pub fn val_i32(&self, v: i32) -> VmError {
            VmError(Cow::Owned(format!("exit_code {v}")))
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
    pub const fn timeout() -> Self { Self(Cow::Borrowed("timeout")) }
    pub const fn absent_leader_nondet_output() -> Self { Self(Cow::Borrowed("absent_leader_nondet_output")) }
    pub const fn host_forbidden() -> Self { Self(Cow::Borrowed("host_forbidden")) }
    pub const fn exit_code() -> __VmError::ExitCode { __VmError::ExitCode }
    pub const fn wasm_trap() -> __VmError::WasmTrap { __VmError::WasmTrap }
    pub const fn out_of() -> __VmError::OutOf { __VmError::OutOf }
    pub const fn fee() -> __VmError::Fee { __VmError::Fee }
    pub const fn evm() -> __VmError::Evm { __VmError::Evm }
    pub const fn invalid_contract() -> __VmError::InvalidContract { __VmError::InvalidContract }
}

#[rustfmt::skip]
impl VmError {
    /// Whether `s` is a well-formed `vm_error` path.
    pub fn is_valid_(s: &str) -> bool {
        if matches!(s,
            "timeout" |
            "absent_leader_nondet_output" |
            "wasm_trap" |
            "wasm_trap unreachable" |
            "wasm_trap stack_overflow" |
            "wasm_trap memory_out_of_bounds" |
            "wasm_trap table_out_of_bounds" |
            "wasm_trap indirect_call_to_null" |
            "wasm_trap bad_signature" |
            "wasm_trap integer_overflow" |
            "wasm_trap integer_divide_by_zero" |
            "wasm_trap bad_conversion_to_integer" |
            "wasm_trap heap_misaligned" |
            "wasm_trap atomic_wait_non_shared_memory" |
            "wasm_trap out_of_fuel" |
            "wasm_trap interrupt" |
            "wasm_trap nondet_instruction" |
            "wasm_trap fault" |
            "out_of memory" |
            "out_of memory wasm_memory" |
            "out_of memory wasm_table" |
            "out_of storage" |
            "out_of receipt nondet_output" |
            "out_of receipt message" |
            "out_of receipt event" |
            "out_of message_fee total" |
            "out_of message_fee node" |
            "out_of vm_recursion" |
            "out_of nondet_blocks" |
            "out_of locked_slots" |
            "out_of upgraders" |
            "out_of fds" |
            "fee no_matching_node" |
            "fee below_minimum" |
            "fee too_many_rounds" |
            "host_forbidden" |
            "evm reverted" |
            "invalid_contract" |
            "invalid_contract absent_runner_comment" |
            "invalid_contract not_utf8_text" |
            "invalid_contract malformed_runner" |
            "invalid_contract major_mismatch" |
            "invalid_contract wasm validating" |
            "invalid_contract wasm linking" |
            "invalid_contract wasm entrypoint"
        ) {
            return true;
        }
        if let Some(rest) = s.strip_prefix("exit_code ") {
            if rest.parse::<i32>().is_ok_and(|v| v.to_string() == rest) {
                return true;
            }
        }
        false
    }
}

pub const EVENT_MAX_TOPICS: u32 = 4;

// EOF
