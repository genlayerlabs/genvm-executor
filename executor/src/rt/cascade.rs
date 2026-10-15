use primitive_types::U256;

use crate::domain::ExecutionEmission;

fn pool_funded_fee(emission: &mut ExecutionEmission) -> Option<&mut U256> {
    match emission {
        ExecutionEmission::InternalMessage {
            message_fee,
            use_balance: false,
            ..
        }
        | ExecutionEmission::InternalDeployMessage {
            message_fee,
            use_balance: false,
            ..
        } => Some(message_fee),
        _ => None,
    }
}

/// Raises each sender-pool internal message to the floor, then adds an equal
/// share of half the remaining budget after fees and external reserves; the
/// other half stays with the parent so one deep path cannot drain the tree
pub fn apply(
    emissions: &mut [ExecutionEmission],
    cascade: &genvm_modules_interfaces::fees::MessageFeeCascade,
) {
    let mut sum = emissions.iter().fold(U256::zero(), |sum, emission| {
        if matches!(emission, ExecutionEmission::ExternalMessage { .. }) {
            sum.saturating_add(cascade.external_fee_reserve)
        } else {
            sum
        }
    });
    let mut count = 0u64;

    for fee in emissions.iter_mut().filter_map(pool_funded_fee) {
        *fee = (*fee).max(cascade.min_primary_fee);
        sum = sum.saturating_add(*fee);
        count += 1;
    }

    if count == 0 || sum >= cascade.budget_remaining {
        return;
    }

    let per_child = (cascade.budget_remaining - sum) / (U256::from(2) * U256::from(count));
    if per_child.is_zero() {
        return;
    }

    // Each fee is at most `sum`, and `per_child` at most `budget_remaining - sum`
    for fee in emissions.iter_mut().filter_map(pool_funded_fee) {
        *fee += per_child;
    }
}

#[cfg(test)]
#[path = "cascade_test.rs"]
mod tests;
