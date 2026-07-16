use bytes::Bytes;
use primitive_types::U256;

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

        fee_params: genlayer_sdk::abi::fees::ExternalMessageParams,
    },
    PostMessage {
        call_key: genlayer_sdk::abi::CallKey,
        address: genlayer_sdk::calldata::Address,
        calldata: genlayer_calldata::codec::Maybe<genlayer_sdk::calldata::Value>,
        value: U256,
        on: genlayer_sdk::abi::gl_call::On,
        message_fee: U256,
        receipt_fee: U256,

        fee_params: genlayer_sdk::abi::fees::InternalMessageParams,
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

        fee_params: genlayer_sdk::abi::fees::InternalMessageParams,
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
