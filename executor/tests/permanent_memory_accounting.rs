use genvm::rt::memlimiter::Limiter;

#[test]
fn uncommitted_allocation_is_released() {
    let limiter = Limiter::with_limit(100);

    let allocation = limiter.reserve_permanent(40).unwrap();
    assert_eq!(limiter.get_remaining_memory(), 60);
    assert_eq!(limiter.get_new_permanent_allocations(), 40);

    drop(allocation);
    assert_eq!(limiter.get_remaining_memory(), 100);
    assert_eq!(limiter.get_new_permanent_allocations(), 0);
}

#[test]
fn committed_allocation_is_retained() {
    let limiter = Limiter::with_limit(100);

    limiter.reserve_permanent(40).unwrap().commit();

    assert_eq!(limiter.get_remaining_memory(), 60);
    assert_eq!(limiter.get_new_permanent_allocations(), 40);
}

#[test]
fn derived_limiter_starts_without_permanent_allocations() {
    let parent = Limiter::with_limit(100);
    parent.reserve_permanent(40).unwrap().commit();

    let child = parent.derived();

    assert_eq!(child.get_remaining_memory(), 60);
    assert_eq!(child.get_new_permanent_allocations(), 0);
}

#[test]
fn nested_folds_transfer_permanent_allocations_once_per_level() {
    let parent = Limiter::with_limit(100);
    let child = parent.derived();
    child.reserve_permanent(30).unwrap().commit();
    let grandchild = child.derived();
    grandchild.reserve_permanent(20).unwrap().commit();

    assert!(child.fold_permanent(&grandchild));
    assert_eq!(child.get_remaining_memory(), 50);
    assert_eq!(child.get_new_permanent_allocations(), 50);

    assert!(parent.fold_permanent(&child));
    assert_eq!(parent.get_remaining_memory(), 50);
    assert_eq!(parent.get_new_permanent_allocations(), 50);
}

#[test]
fn failed_fold_keeps_propagated_parent_counter() {
    let parent = Limiter::with_limit(10);
    let child = Limiter::with_limit(20);
    child.reserve_permanent(15).unwrap().commit();

    assert!(!parent.fold_permanent(&child));
    assert_eq!(parent.get_remaining_memory(), 10);
    assert_eq!(parent.get_new_permanent_allocations(), 15);
}
