use bytes::Bytes;
use primitive_types::U256;

pub mod fees;

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
    },
    EmitEvent {
        topics: Vec<Bytes>,
        blob: genlayer_calldata::codec::Maybe<
            genlayer_sdk::calldata::Map<genlayer_sdk::calldata::Value>,
        >,
        storage_fee: U256,
    },
}
