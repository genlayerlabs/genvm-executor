use serde::Serialize;

use crate::{public_abi, runners};

#[derive(Clone, Serialize, genlayer_calldata::Encode)]
pub struct Permissions {
    pub deterministic: bool,
    pub write_storage: bool,
    pub send_messages: bool,
    pub call_others: bool,
    pub spawn_nondet: bool,
    pub can_use_balance_for_message_fees: bool,
}

#[derive(Clone, Serialize, genlayer_calldata::Encode)]
pub struct Execution {
    pub state_mode: public_abi::StorageType,
    pub topmost_runner_id: runners::Id,
}

#[derive(Clone, Serialize, genlayer_calldata::Encode)]
pub struct Config {
    pub needs_error_fingerprint: bool,
    pub permissions: Permissions,
    pub execution: Execution,
}

impl Config {
    pub fn is_main(&self) -> bool {
        self.execution.state_mode == public_abi::StorageType::Default
    }
}
