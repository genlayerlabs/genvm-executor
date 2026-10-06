use genlayer_sdk::abi;
use primitive_types::U256;
use std::collections::BTreeSet;

use super::message::internal_params_valid;
use crate::rt;

const DOMAIN: [u8; 32] = [
    0x5f, 0x77, 0x18, 0x1a, 0x28, 0xfb, 0x88, 0xdd, 0x05, 0x97, 0x39, 0x0f, 0x85, 0x2a, 0x7f, 0x17,
    0x4b, 0xad, 0x95, 0x30, 0x3d, 0xef, 0xee, 0xfb, 0x5b, 0xa8, 0x06, 0xaa, 0xa3, 0xef, 0x64, 0xfb,
];

pub(super) const ROOT_PARENT: U256 = U256::MAX;
const MAX_NODES: usize = 24;
const MAX_FEE_PARAMS_BYTES: usize = 1_024;
const INTERNAL_FEE_PARAMS_BASE_BYTES: usize = 320;
const MAX_INTERNAL_ROTATIONS: usize = (MAX_FEE_PARAMS_BYTES - INTERNAL_FEE_PARAMS_BASE_BYTES) / 32;
const MAX_GRANT_BYTES: usize = 31_712;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GrantPolicy {
    #[cfg(test)]
    Closed = 0,
    Open = 1,
    Pinned = 2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AllocationFeeParams {
    External(abi::fees::ExternalMessageParams),
    Internal(abi::fees::InternalMessageParams),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FlatAllocation {
    pub on_acceptance: bool,
    pub parent_index: U256,
    pub recipient: genlayer_calldata::Address,
    pub call_key: abi::CallKey,
    pub budget: U256,
    pub fee_params: AllocationFeeParams,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Grant {
    pub policy: GrantPolicy,
    pub budget: U256,
    pub allocations: Vec<FlatAllocation>,
}

#[derive(Debug)]
pub(super) enum GrantValidationError {
    Inval,
    Budget,
    Tree,
    External,
    Internal(rt::errors::Error),
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct PreparedGrant {
    pub budget: U256,
    pub subtree: Vec<u8>,
}

fn push_word(output: &mut Vec<u8>, value: U256) {
    output.extend_from_slice(&value.to_big_endian());
}

fn push_usize(output: &mut Vec<u8>, value: usize) {
    push_word(output, U256::from(value));
}

fn push_address(output: &mut Vec<u8>, address: genlayer_calldata::Address) {
    output.extend_from_slice(&[0; 12]);
    output.extend_from_slice(&address.raw());
}

fn encode_bytes(value: &[u8]) -> Vec<u8> {
    let padded_len = value.len().div_ceil(32) * 32;
    let mut output = Vec::with_capacity(32 + padded_len);
    push_usize(&mut output, value.len());
    output.extend_from_slice(value);
    output.resize(32 + padded_len, 0);
    output
}

fn encode_internal_fee_params(params: &abi::fees::InternalMessageParams) -> Vec<u8> {
    let mut output = Vec::with_capacity(320 + params.rotations.len() * 32);
    push_usize(&mut output, 32);
    push_word(&mut output, params.leader_time_units_allocation);
    push_word(&mut output, params.validator_time_units_allocation);
    push_usize(
        &mut output,
        params
            .rotations
            .len()
            .checked_sub(1)
            .expect("validated rotations must be non-empty"),
    );
    push_word(&mut output, params.execution_budget_per_round);
    push_usize(&mut output, 8 * 32);
    push_word(&mut output, params.max_price_gen_per_time_unit);
    push_word(&mut output, params.storage_fee_max_gas_price);
    push_word(&mut output, params.receipt_fee_max_gas_price);
    push_usize(&mut output, params.rotations.len());
    for rotation in &params.rotations {
        push_word(&mut output, *rotation);
    }
    output
}

fn encode_external_fee_params(params: &abi::fees::ExternalMessageParams) -> Vec<u8> {
    let mut output = Vec::with_capacity(64);
    push_word(&mut output, params.gas_limit);
    push_word(&mut output, params.max_gas_price);
    output
}

fn encode_allocation(node: &FlatAllocation) -> Vec<u8> {
    let (message_type, fee_params) = match &node.fee_params {
        AllocationFeeParams::External(params) => (0, encode_external_fee_params(params)),
        AllocationFeeParams::Internal(params) => (1, encode_internal_fee_params(params)),
    };
    let mut output = Vec::with_capacity(256 + fee_params.len().next_multiple_of(32));
    push_usize(&mut output, message_type);
    push_usize(&mut output, usize::from(node.on_acceptance));
    push_word(&mut output, node.parent_index);
    push_address(&mut output, node.recipient);
    output.extend_from_slice(&node.call_key.0);
    push_word(&mut output, node.budget);
    push_usize(&mut output, 7 * 32);
    output.extend_from_slice(&encode_bytes(&fee_params));
    output
}

pub(super) fn encode_grant(grant: &Grant) -> Vec<u8> {
    let encoded_nodes = grant
        .allocations
        .iter()
        .map(encode_allocation)
        .collect::<Vec<_>>();

    let mut allocations = Vec::new();
    push_usize(&mut allocations, encoded_nodes.len());
    let mut offset = encoded_nodes.len() * 32;
    for node in &encoded_nodes {
        push_usize(&mut allocations, offset);
        offset += node.len();
    }
    for node in encoded_nodes {
        allocations.extend_from_slice(&node);
    }

    let mut output = Vec::with_capacity(64 + 128 + allocations.len());
    output.extend_from_slice(&DOMAIN);
    push_usize(&mut output, 64);
    push_usize(&mut output, 1);
    push_usize(&mut output, grant.policy as usize);
    push_word(&mut output, grant.budget);
    push_usize(&mut output, 4 * 32);
    output.extend_from_slice(&allocations);
    output
}

fn sibling_key(node: &abi::gl_call::AllocationNode) -> (u8, [u8; 20], [u8; 32]) {
    match node {
        abi::gl_call::AllocationNode::Internal(node) => (1, node.recipient.raw(), node.call_key.0),
        abi::gl_call::AllocationNode::External(node) => (0, node.recipient.raw(), node.call_key.0),
    }
}

fn validate_nodes<F>(
    nodes: &[abi::gl_call::AllocationNode],
    enclosing_rotations: usize,
    quote: &mut F,
) -> Result<Grant, GrantValidationError>
where
    F: FnMut(&abi::fees::InternalMessageParams) -> rt::errors::Result<U256>,
{
    if nodes.len() > MAX_NODES {
        return Err(GrantValidationError::Tree);
    }
    let mut budget = U256::zero();
    let mut children_budgets = vec![U256::zero(); nodes.len()];
    let mut sibling_keys = BTreeSet::new();
    for (index, node) in nodes.iter().enumerate() {
        let (parent_index, node_budget) = match node {
            abi::gl_call::AllocationNode::Internal(node) => (node.parent_index, node.budget),
            abi::gl_call::AllocationNode::External(node) => (node.parent_index, node.budget),
        };
        if node_budget.is_zero() {
            return Err(GrantValidationError::Budget);
        }
        if !sibling_keys.insert((parent_index, sibling_key(node))) {
            return Err(GrantValidationError::Tree);
        }
        let total = if parent_index == ROOT_PARENT {
            &mut budget
        } else {
            if parent_index >= U256::from(index)
                || !matches!(
                    nodes[parent_index.as_usize()],
                    abi::gl_call::AllocationNode::Internal(_)
                )
                || matches!(node, abi::gl_call::AllocationNode::External(_))
            {
                return Err(GrantValidationError::Tree);
            }
            &mut children_budgets[parent_index.as_usize()]
        };
        *total = total
            .checked_add(node_budget)
            .ok_or(GrantValidationError::Budget)?;
    }

    let mut allocations = Vec::with_capacity(nodes.len());
    for (index, node) in nodes.iter().enumerate() {
        match node {
            abi::gl_call::AllocationNode::Internal(node) => {
                if !internal_params_valid(&node.fee_params)
                    || node.fee_params.rotations.len() > MAX_INTERNAL_ROTATIONS
                {
                    return Err(GrantValidationError::Inval);
                }

                allocations.push(FlatAllocation {
                    on_acceptance: matches!(node.on, abi::gl_call::On::Decided),
                    parent_index: node.parent_index,
                    recipient: node.recipient,
                    call_key: node.call_key,
                    budget: node.budget,
                    fee_params: AllocationFeeParams::Internal(node.fee_params.clone()),
                });
                let parent_rotations = if node.parent_index == ROOT_PARENT {
                    enclosing_rotations
                } else {
                    let abi::gl_call::AllocationNode::Internal(parent) =
                        &nodes[node.parent_index.as_usize()]
                    else {
                        unreachable!("parent type validated above")
                    };
                    parent.fee_params.rotations.len()
                };
                let primary = quote(&node.fee_params).map_err(GrantValidationError::Internal)?;
                let required = primary
                    .checked_add(children_budgets[index])
                    .and_then(|amount| {
                        if matches!(node.on, abi::gl_call::On::Decided) {
                            amount.checked_mul(U256::from(parent_rotations))
                        } else {
                            Some(amount)
                        }
                    })
                    .ok_or(GrantValidationError::Budget)?;
                if node.budget < required {
                    return Err(GrantValidationError::Budget);
                }
            }
            abi::gl_call::AllocationNode::External(node) => {
                // The transaction-pinned minimum external gas limit is not available in host input yet.
                let unit = node
                    .fee_params
                    .gas_limit
                    .checked_mul(node.fee_params.max_gas_price)
                    .filter(|unit| !unit.is_zero())
                    .ok_or(GrantValidationError::External)?;
                if node.budget < unit || node.budget % unit != U256::zero() {
                    return Err(GrantValidationError::External);
                }
                allocations.push(FlatAllocation {
                    on_acceptance: false,
                    parent_index: node.parent_index,
                    recipient: node.recipient,
                    call_key: node.call_key,
                    budget: node.budget,
                    fee_params: AllocationFeeParams::External(node.fee_params),
                });
            }
        }
    }
    Ok(Grant {
        policy: GrantPolicy::Pinned,
        budget,
        allocations,
    })
}

pub(super) fn prepare_descendants<F>(
    enclosing_params: &abi::fees::InternalMessageParams,
    descendants: Option<abi::gl_call::Descendants>,
    mut quote: F,
) -> Result<PreparedGrant, GrantValidationError>
where
    F: FnMut(&abi::fees::InternalMessageParams) -> rt::errors::Result<U256>,
{
    let grant = match descendants {
        None => {
            return Ok(PreparedGrant {
                budget: U256::zero(),
                subtree: vec![],
            });
        }
        Some(abi::gl_call::Descendants::Open(budget)) if budget.is_zero() => {
            return Ok(PreparedGrant {
                budget: U256::zero(),
                subtree: vec![],
            });
        }
        Some(abi::gl_call::Descendants::Pinned(ref nodes)) if nodes.is_empty() => {
            return Ok(PreparedGrant {
                budget: U256::zero(),
                subtree: vec![],
            });
        }
        Some(abi::gl_call::Descendants::Open(budget)) => Grant {
            policy: GrantPolicy::Open,
            budget,
            allocations: vec![],
        },
        Some(abi::gl_call::Descendants::Pinned(nodes)) => {
            validate_nodes(&nodes, enclosing_params.rotations.len(), &mut quote)?
        }
    };

    let subtree = encode_grant(&grant);
    debug_assert!(subtree.len() <= MAX_GRANT_BYTES);
    Ok(PreparedGrant {
        budget: grant.budget,
        subtree,
    })
}
