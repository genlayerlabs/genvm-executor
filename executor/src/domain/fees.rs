use genlayer_sdk::abi::gl_call::On;
use primitive_types::U256;

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    genlayer_calldata::Encode,
    genlayer_calldata::Decode,
)]
pub struct InternalMessageParams {
    pub leader_timeunits_allocation: U256,
    pub validator_timeunits_allocation: U256,
    pub execution_budget_per_round: U256,
    /// Per-round rotation allocations; `rotations[0]` is the initial round, the
    /// rest are appeal rounds. Must be non-empty.
    ///
    /// The fee evaluator derives `appeal_rounds = rotations.len() - 1`.
    pub rotations: Vec<U256>,
    /// Per-time-unit GEN price cap locked at activation (consensus CON-549,
    /// v0.6-dev). The chain charges at this cap as the funding multiplier and
    /// cancels the tx if the global price exceeds it; `MessagePayments` requires
    /// it to be non-zero for internal messages.
    pub max_price_gen_per_time_unit: U256,
    /// Max gas price applied to the storage-fee component (v0.6-dev). Revert
    /// guard in `_calculateRoundFees`; must be non-zero for internal messages.
    pub storage_fee_max_gas_price: U256,
    /// Max gas price applied to the receipt-fee component (v0.6-dev). Revert
    /// guard in `_calculateRoundFees`; must be non-zero for internal messages.
    pub receipt_fee_max_gas_price: U256,
}

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

/// One allocation available to messages emitted by this execution.
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
    /// `None` means any call key; the node converts the chain's wildcard sentinel.
    pub call_key: Option<genlayer_sdk::abi::CallKey>,
    /// Available allowance; zero is exhausted, `None` is uncapped.
    pub budget: Option<U256>,
    pub on: On,
    /// Same structure as TX-level params.
    pub fee_params: MessageAllocationNodeParams,
    pub children_budget: U256,
    pub subtree: bytes::Bytes,
}

impl MessageAllocationNode {
    #[allow(clippy::if_same_then_else)]
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

    #[allow(clippy::if_same_then_else)]
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
