//! Guest-facing message-fee parameter types.
//!
//! [`InternalMessageParams`] lives here (rather than in `genvm_common`) so that
//! [`gl_call`](super::gl_call) can reference it: a guest emitting a
//! balance-funded internal message ships these params, and GenVM meters the fee
//! from them.

use primitive_types::U256;

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
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    genlayer_calldata::Encode,
    genlayer_calldata::Decode,
)]
#[cfg_attr(feature = "fuzzing", derive(arbitrary::Arbitrary))]
pub struct InternalMessageParams {
    #[cfg_attr(feature = "fuzzing", arbitrary(with = crate::abi::arb::arb_u256))]
    pub leader_time_units_allocation: U256,
    #[cfg_attr(feature = "fuzzing", arbitrary(with = crate::abi::arb::arb_u256))]
    pub validator_time_units_allocation: U256,
    #[cfg_attr(feature = "fuzzing", arbitrary(with = crate::abi::arb::arb_u256))]
    pub execution_budget_per_round: U256,
    /// Per-round rotation allocations; `rotations[0]` is the initial round, the
    /// rest are appeal rounds. Must be non-empty.
    ///
    /// The chain's `InternalMessageFeeParams` carries an explicit `appealRounds`
    /// field, but it is not stored here: the chain enforces
    /// `appealRounds == rotations.length - 1` (`FeesVerifier.InvalidAppealRounds`),
    /// so we derive `appeal_rounds = rotations.len() - 1` instead. The ABI
    /// encoder re-inserts it so the encoded bytes (and their `keccak256` fee-param
    /// pin) match the chain's field layout.
    #[cfg_attr(feature = "fuzzing", arbitrary(with = crate::abi::arb::arb_vec_u256))]
    pub rotations: Vec<U256>,
    /// Per-time-unit GEN price cap locked at activation (consensus CON-549,
    /// v0.6-dev). The chain charges at this cap as the funding multiplier and
    /// cancels the tx if the global price exceeds it; `MessagePayments` requires
    /// it to be non-zero for internal messages.
    #[cfg_attr(feature = "fuzzing", arbitrary(with = crate::abi::arb::arb_u256))]
    pub max_price_gen_per_time_unit: U256,
    /// Max gas price applied to the storage-fee component (v0.6-dev). Revert
    /// guard in `_calculateRoundFees`; must be non-zero for internal messages.
    #[cfg_attr(feature = "fuzzing", arbitrary(with = crate::abi::arb::arb_u256))]
    pub storage_fee_max_gas_price: U256,
    /// Max gas price applied to the receipt-fee component (v0.6-dev). Revert
    /// guard in `_calculateRoundFees`; must be non-zero for internal messages.
    #[cfg_attr(feature = "fuzzing", arbitrary(with = crate::abi::arb::arb_u256))]
    pub receipt_fee_max_gas_price: U256,
}
