use genvm::rt::errors::{Error, ErrorKind};
use genvm::rt::memlimiter::Limiter;
use genvm_common::internal_constants::memory_limiter_consts::TABLE_ENTRY;
use wasmtime::ResourceLimiter;

const PAGE: usize = 65536;
const BUDGET: u32 = 1 << 20;

fn assert_internal(err: wasmtime::Error) {
    let err = err
        .downcast_ref::<Error>()
        .unwrap_or_else(|| panic!("expected executor error, got {err:?}"));
    assert!(
        matches!(err.kind, ErrorKind::Internal),
        "expected internal error, got {err}"
    );
}

#[test]
fn explicit_spawn_limit_is_honored() {
    let limiter = Limiter::with_limit(100);
    assert_eq!(limiter.get_remaining_memory(), 100);
    assert!(limiter.consume(40));
    assert_eq!(limiter.get_remaining_memory(), 60);
    assert!(!limiter.consume(61));
}

#[test]
fn memory_growth_past_maximum_is_refused_without_charge() {
    let mut limiter = Limiter::with_limit(BUDGET);
    let granted = limiter
        .memory_growing(PAGE, 3 * PAGE, Some(2 * PAGE))
        .unwrap();
    assert!(!granted);
    assert_eq!(limiter.get_remaining_memory(), BUDGET);
}

#[test]
fn memory_growth_up_to_maximum_is_charged() {
    let mut limiter = Limiter::with_limit(BUDGET);
    let granted = limiter
        .memory_growing(PAGE, 2 * PAGE, Some(2 * PAGE))
        .unwrap();
    assert!(granted);
    assert_eq!(limiter.get_remaining_memory(), BUDGET - PAGE as u32);
}

#[test]
fn table_growth_past_maximum_is_refused_without_charge() {
    let mut limiter = Limiter::with_limit(BUDGET);
    let granted = limiter.table_growing(1, 3, Some(2)).unwrap();
    assert!(!granted);
    assert_eq!(limiter.get_remaining_memory(), BUDGET);
}

#[test]
fn table_growth_up_to_maximum_is_charged() {
    let mut limiter = Limiter::with_limit(BUDGET);
    let granted = limiter.table_growing(1, 2, Some(2)).unwrap();
    assert!(granted);
    assert_eq!(limiter.get_remaining_memory(), BUDGET - TABLE_ENTRY);
}

#[test]
fn memory_grow_failure_is_internal_error() {
    let mut limiter = Limiter::with_limit(BUDGET);
    let err = limiter
        .memory_grow_failed(wasmtime::format_err!("mprotect: ENOMEM"))
        .unwrap_err();
    assert_internal(err);
}

#[test]
fn table_grow_failure_is_internal_error() {
    let mut limiter = Limiter::with_limit(BUDGET);
    let err = limiter
        .table_grow_failed(wasmtime::format_err!("allocation failed"))
        .unwrap_err();
    assert_internal(err);
}
