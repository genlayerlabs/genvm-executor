use bytes::Bytes;
use primitive_types::U256;

pub use genlayer_sdk::abi::entry::MessageData;

pub mod fees;

fn default_record_actions() -> Vec<String> {
    Vec::new()
}

#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, PartialEq, Eq, genlayer_calldata::Encode)]
#[calldata(tag = "type")]
pub enum ExecutionEmission {
    EthSend {
        address: genlayer_sdk::calldata::Address,
        calldata: Bytes,
        value: U256,
        message_fee: U256,
        receipt_fee: U256,

        fee_params: fees::ExternalMessageParams,
    },
    PostMessage {
        call_key: genlayer_sdk::abi::CallKey,
        address: genlayer_sdk::calldata::Address,
        calldata: genlayer_calldata::codec::Maybe<genlayer_sdk::calldata::Value>,
        value: U256,
        on: genlayer_sdk::abi::gl_call::On,
        message_fee: U256,
        receipt_fee: U256,

        fee_params: fees::InternalMessageParams,
        subtree: bytes::Bytes,
        /// Chain `useBalance`: the fee is drawn from the emitting contract's
        /// balance rather than the sender's prefunded message-fee pool.
        use_balance: bool,
    },
    DeployContract {
        calldata: genlayer_calldata::codec::Maybe<genlayer_sdk::calldata::Value>,
        code: Bytes,
        value: U256,
        on: genlayer_sdk::abi::gl_call::On,
        salt_nonce: U256,
        message_fee: U256,
        receipt_fee: U256,

        fee_params: fees::InternalMessageParams,
        subtree: bytes::Bytes,
        /// Chain `useBalance`; see `ExecutionEmission::PostMessage::use_balance`.
        use_balance: bool,
    },
    EmitEvent {
        topics: Vec<Bytes>,
        blob: genlayer_calldata::codec::Maybe<
            genlayer_sdk::calldata::Map<genlayer_sdk::calldata::Value>,
        >,
        storage_fee: U256,
    },
}

#[derive(
    Debug,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    genlayer_calldata::Encode,
    genlayer_calldata::Decode,
)]
pub struct ExecutionData {
    pub calldata: Bytes,
    pub message: MessageData,
    pub host_data: String,
    pub code: Option<Bytes>,
    pub leader_nondet_results: Option<Vec<Bytes>>,
    /// Maps each host method (by index) to a host id. When empty, all methods use host 0.
    pub method_hosts: Vec<u8>,
    pub bucket_totals: Vec<num_bigint::BigInt>,
    /// Host-provided `node` fee constants (moved off `host_data`).
    pub gas_data: std::collections::BTreeMap<String, String>,
    /// Message-fee allocation tree passed alongside the execution.
    pub message_fee_allocation: Vec<fees::MessageAllocationNode>,
    /// Initial time-unit budget for this execution.
    pub initial_time_units_allocation: u32,
    /// Auditable supervisor action kinds to return in the execution result.
    #[serde(default)]
    #[calldata(default = default_record_actions)]
    pub record_actions: Vec<String>,
}
