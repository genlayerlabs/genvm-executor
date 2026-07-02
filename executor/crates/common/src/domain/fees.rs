use genlayer_sdk::abi::gl_call::On;
use primitive_types::U256;

mod abi;

/// Balance-funded internal messages carry these params from the guest, so the
/// type is guest-facing and lives in sdk-rs; re-export it here to keep the
/// executor-side `genvm_common::domain::fees::InternalMessageParams` path stable.
pub use genlayer_sdk::abi::fees::InternalMessageParams;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    genlayer_calldata::Encode,
    genlayer_calldata::Decode,
)]
pub struct ExternalMessageParams {
    pub gas_limit: U256,
    pub max_gas_price: U256,
}

#[derive(
    Debug,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    genlayer_calldata::Encode,
    genlayer_calldata::Decode,
)]
pub enum MessageAllocationNodeParams {
    Internal(std::sync::Arc<InternalMessageParams>),
    External(ExternalMessageParams),
}

/// One node of the message-fee allocation tree.
#[derive(
    Debug,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    genlayer_calldata::Encode,
    genlayer_calldata::Decode,
)]
pub struct MessageAllocationNode {
    /// Target contract address; `None` means wildcard (any recipient).
    pub recipient: Option<genlayer_sdk::calldata::Address>,
    /// `None` = wildcard: all call keys for this recipient
    /// (chain sentinel: `CALL_KEY_WILDCARD` = `bytes32(0)`).
    pub call_key: Option<genlayer_sdk::abi::CallKey>,
    /// Max budget for matching messages.
    pub budget: U256,
    pub on: On,
    /// Same structure as TX-level params.
    pub fee_params: MessageAllocationNodeParams,
    pub children: Vec<MessageAllocationNode>,
}

impl MessageAllocationNode {
    /// ABI-encodes the nested allocation tree as the chain's flat
    /// `MessageAllocationNode[]` representation (matching `abi.encode(nodes)`),
    /// flattening children to parent-pointer form via pre-order traversal so that
    /// every parent precedes its children and their `parentIndex` is well-defined.
    pub fn abi_encode(roots: &[MessageAllocationNode]) -> Vec<u8> {
        abi::encode(roots)
    }

    pub fn matches_internal(
        &self,
        on: On,
        recipient: genlayer_sdk::calldata::Address,
        call_key: genlayer_sdk::abi::CallKey,
    ) -> Option<std::sync::Arc<InternalMessageParams>> {
        let MessageAllocationNodeParams::Internal(params) = &self.fee_params else {
            return None;
        };
        if on != self.on {
            None
        } else if self.recipient.as_ref().is_some_and(|r| *r != recipient) {
            None
        } else if self.call_key.as_ref().is_some_and(|ck| *ck != call_key) {
            None
        } else {
            Some(params.clone())
        }
    }

    pub fn matches_external(
        &self,
        recipient: genlayer_sdk::calldata::Address,
        call_key: genlayer_sdk::abi::CallKey,
    ) -> Option<ExternalMessageParams> {
        let MessageAllocationNodeParams::External(params) = &self.fee_params else {
            return None;
        };
        if self.recipient.as_ref().is_some_and(|r| *r != recipient) {
            None
        } else if self.call_key.as_ref().is_some_and(|ck| *ck != call_key) {
            None
        } else {
            Some(*params)
        }
    }
}
