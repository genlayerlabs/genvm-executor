use primitive_types::U256;

use crate::domain::ExecutionEmission;

fn internal_fee(emission: &mut ExecutionEmission) -> Option<&mut U256> {
    match emission {
        ExecutionEmission::PostMessage { message_fee, .. }
        | ExecutionEmission::DeployContract { message_fee, .. } => Some(message_fee),
        _ => None,
    }
}

/// Raises each internal message to the floor, then adds an equal share of half
/// of `budget_total - Σ fees`; the other half stays with the parent so one deep
/// path cannot drain the tree
pub fn apply(
    emissions: &mut [ExecutionEmission],
    cascade: &genvm_modules_interfaces::fees::MessageFeeCascade,
) {
    let mut sum = U256::zero();
    let mut count = 0u64;

    for fee in emissions.iter_mut().filter_map(internal_fee) {
        *fee = (*fee).max(cascade.min_primary_fee);
        sum = sum.saturating_add(*fee);
        count += 1;
    }

    if count == 0 || sum >= cascade.budget_total {
        return;
    }

    let per_child = (cascade.budget_total - sum) / (U256::from(2) * U256::from(count));
    if per_child.is_zero() {
        return;
    }

    // Each fee is at most `sum`, and `per_child` at most `budget_total - sum`
    for fee in emissions.iter_mut().filter_map(internal_fee) {
        *fee += per_child;
    }
}

#[cfg(test)]
#[path = "cascade_test.rs"]
mod tests;
