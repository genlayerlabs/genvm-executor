use genvm::calldata;
use genvm::config::{FeesBucketConfig, FeesConfig};
use genvm::rt;
use genvm::rt::vm::storage::{HostStorage, HostStorageLocking, Limiter, PageID, Storage};
use genvm::SlotID;
use genvm_common::{internal_constants::memory_limiter_consts, sync};

#[derive(Clone)]
struct FakeHost(std::sync::Arc<Vec<u8>>);

struct FakeLock(std::sync::Arc<Vec<u8>>);

impl HostStorage for FakeLock {
    fn storage_read(&mut self, _slot_id: SlotID, index: u32, buf: &mut [u8]) -> anyhow::Result<()> {
        let data = &self.0;
        for (i, out) in buf.iter_mut().enumerate() {
            *out = data.get(index as usize + i).copied().unwrap_or(0);
        }
        Ok(())
    }
}

impl HostStorageLocking for FakeHost {
    type ReturnType<'a> = FakeLock;

    async fn lock(&self) -> FakeLock {
        FakeLock(self.0.clone())
    }
}

/// A minimal `DataLimit` whose storage bucket charges one unit per page.
fn data_fees(total_pages: u64) -> Limiter {
    let bucket = |delta: &str| FeesBucketConfig {
        buckets: vec![symbol_table::GlobalSymbol::from("test")],
        subtract_on_start_expr: "0".to_owned(),
        delta_expr: delta.to_owned(),
    };
    let fees = FeesConfig {
        expr_prelude: String::new(),
        storage: bucket("\\attrs = attrs.pages"),
        message_receipt: bucket("\\attrs = 0"),
        nondet_output: bucket("\\attrs = 0"),
        message_fee: bucket("\\attrs = 0"),
        event: bucket("\\attrs = 0"),
    };
    let dl = rt::fees::DataLimit::new(
        std::collections::HashMap::from([(
            "test".to_owned(),
            primitive_types::U256::from(total_pages),
        )]),
        fees,
        Default::default(),
    )
    .unwrap();
    Limiter::new(sync::DArc::new(dl))
}

fn storage_with_budget(
    total_pages: u64,
    memory_budget: u32,
) -> (Storage<FakeHost>, rt::memlimiter::Limiter) {
    let mem = rt::memlimiter::Limiter::with_limit(memory_budget);
    let storage = Storage::new(
        calldata::Address::zero(),
        data_fees(total_pages),
        mem.clone(),
        FakeHost(std::sync::Arc::new(Vec::new())),
    );
    (storage, mem)
}

// -- storage page memory charging -----------------------------------

#[tokio::test]
async fn new_page_consumes_exact_memory_charge() {
    let charge = memory_limiter_consts::NEW_STORAGE_PAGE;
    let (mut storage, mem) = storage_with_budget(1, charge + 1);

    storage
        .write_page(PageID(SlotID::ZERO, 0), [1; 32])
        .await
        .unwrap();

    assert_eq!(mem.get_remaining_memory(), 1);
}

#[tokio::test]
async fn rewriting_page_consumes_no_more_memory() {
    let charge = memory_limiter_consts::NEW_STORAGE_PAGE;
    let (mut storage, mem) = storage_with_budget(1, charge);
    let page = PageID(SlotID::ZERO, 0);

    storage.write_page(page, [1; 32]).await.unwrap();
    storage.write_page(page, [2; 32]).await.unwrap();

    assert_eq!(mem.get_remaining_memory(), 0);
    assert_eq!(storage.read_page_override(page), Some([2; 32]));
}

#[tokio::test]
async fn page_rejected_by_memory_reports_out_of_memory_before_storage() {
    let charge = memory_limiter_consts::NEW_STORAGE_PAGE;
    let (mut storage, mem) = storage_with_budget(0, charge - 1);

    let err = storage
        .write_page(PageID(SlotID::ZERO, 0), [1; 32])
        .await
        .unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("out_of memory"),
        "unexpected error: {message}"
    );
    assert!(
        !message.contains("out_of storage"),
        "unexpected error: {message}"
    );
    assert_eq!(mem.get_remaining_memory(), charge - 1);
    assert_eq!(storage.pages_len(), 0);
}

#[tokio::test]
async fn page_rejected_by_memory_leaves_storage_fee_available() {
    let charge = memory_limiter_consts::NEW_STORAGE_PAGE;
    let (mut storage, _) = storage_with_budget(1, charge - 1);
    let page = PageID(SlotID::ZERO, 0);

    storage.write_page(page, [1; 32]).await.unwrap_err();

    let child_mem = rt::memlimiter::Limiter::with_limit(charge);
    let mut child = storage.fork(child_mem.clone()).unwrap();
    child.write_page(page, [1; 32]).await.unwrap();
    assert_eq!(child_mem.get_remaining_memory(), 0);
}

#[tokio::test]
async fn page_rejected_by_storage_fee_leaves_memory_available() {
    let charge = memory_limiter_consts::NEW_STORAGE_PAGE;
    let (mut storage, mem) = storage_with_budget(0, charge);

    let err = storage
        .write_page(PageID(SlotID::ZERO, 0), [1; 32])
        .await
        .unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("out_of storage"),
        "unexpected error: {message}"
    );
    assert_eq!(mem.get_remaining_memory(), charge);
    assert_eq!(storage.pages_len(), 0);
}

#[tokio::test]
async fn pages_present_before_fork_are_not_charged_during_fold() {
    let charge = memory_limiter_consts::NEW_STORAGE_PAGE;
    let (mut parent, parent_mem) = storage_with_budget(1, charge + 1);
    let page = PageID(SlotID::ZERO, 0);
    parent.write_page(page, [1; 32]).await.unwrap();

    // The child affords exactly the page it inherits and writes nothing itself.
    let child_mem =
        rt::memlimiter::Limiter::with_limit(memory_limiter_consts::STORAGE_PAGE_INHERITED);
    let child = parent.fork(child_mem).unwrap();
    parent.fold(child).unwrap();

    assert_eq!(parent_mem.get_remaining_memory(), 1);
    assert_eq!(parent.read_page_override(page), Some([1; 32]));
}

#[tokio::test]
async fn folding_child_charges_only_child_pages_and_adopts_them() {
    let charge = memory_limiter_consts::NEW_STORAGE_PAGE;
    let budget = 2 * charge;
    let (mut parent, parent_mem) = storage_with_budget(2, budget);
    let child_mem = rt::memlimiter::Limiter::with_limit(budget);
    let mut child = parent.fork(child_mem).unwrap();
    let first = PageID(SlotID::ZERO, 0);
    let second = PageID(SlotID::ZERO, 1);
    child.write_page(first, [1; 32]).await.unwrap();
    child.write_page(second, [2; 32]).await.unwrap();

    parent.fold(child).unwrap();

    assert_eq!(parent_mem.get_remaining_memory(), 0);
    assert_eq!(parent.pages_len(), 2);
    assert_eq!(parent.read_page_override(first), Some([1; 32]));
    assert_eq!(parent.read_page_override(second), Some([2; 32]));
}

#[tokio::test]
async fn nested_folds_transfer_pages_once_at_each_level() {
    let charge = memory_limiter_consts::NEW_STORAGE_PAGE;
    let budget = 2 * charge;
    let (mut parent, parent_mem) = storage_with_budget(2, budget);
    let child_mem = rt::memlimiter::Limiter::with_limit(budget);
    let mut child = parent.fork(child_mem.clone()).unwrap();
    let grandchild_mem = rt::memlimiter::Limiter::with_limit(budget);
    let mut grandchild = child.fork(grandchild_mem.clone()).unwrap();
    let first = PageID(SlotID::ZERO, 0);
    let second = PageID(SlotID::ZERO, 1);
    grandchild.write_page(first, [1; 32]).await.unwrap();
    grandchild.write_page(second, [2; 32]).await.unwrap();

    child.fold(grandchild).unwrap();
    parent.fold(child).unwrap();

    assert_eq!(grandchild_mem.get_remaining_memory(), 0);
    assert_eq!(child_mem.get_remaining_memory(), 0);
    assert_eq!(parent_mem.get_remaining_memory(), 0);
    assert_eq!(parent.pages_len(), 2);
    assert_eq!(parent.read_page_override(first), Some([1; 32]));
    assert_eq!(parent.read_page_override(second), Some([2; 32]));
}

#[tokio::test]
async fn page_count_tracks_distinct_pages_not_writes() {
    let charge = memory_limiter_consts::NEW_STORAGE_PAGE;
    let (mut storage, _) = storage_with_budget(2, 2 * charge);
    let first = PageID(SlotID::ZERO, 0);
    let second = PageID(SlotID::ZERO, 1);

    storage.write_page(first, [1; 32]).await.unwrap();
    storage.write_page(first, [2; 32]).await.unwrap();
    storage.write_page(second, [3; 32]).await.unwrap();
    storage.write_page(second, [4; 32]).await.unwrap();

    assert_eq!(storage.pages_len(), 2);
}

// -- inherited pages -------------------------------------------------

/// Two pages written into a fresh storage, ready to be inherited.
async fn storage_of_two_pages() -> Storage<FakeHost> {
    let (mut storage, _) = storage_with_budget(2, 2 * memory_limiter_consts::NEW_STORAGE_PAGE);
    storage
        .write_page(PageID(SlotID::ZERO, 0), [1; 32])
        .await
        .unwrap();
    storage
        .write_page(PageID(SlotID::ZERO, 1), [2; 32])
        .await
        .unwrap();
    storage
}

#[tokio::test]
async fn forking_charges_the_child_for_every_inherited_page() {
    let storage = storage_of_two_pages().await;
    let charge = 2 * memory_limiter_consts::STORAGE_PAGE_INHERITED;
    let child_mem = rt::memlimiter::Limiter::with_limit(charge);

    storage.fork(child_mem.clone()).unwrap();

    assert_eq!(child_mem.get_remaining_memory(), 0);
}

#[tokio::test]
async fn forking_beyond_the_child_budget_is_rejected() {
    let storage = storage_of_two_pages().await;
    let charge = 2 * memory_limiter_consts::STORAGE_PAGE_INHERITED;
    let child_mem = rt::memlimiter::Limiter::with_limit(charge - 1);

    assert!(storage.fork(child_mem).is_err());
}

/// A storage built by `new` is inherited from nobody -- nothing else holds a
/// version of its map -- so it must carry no inherited charge. The root VM's
/// deploy-time writes would otherwise be billed twice.
#[tokio::test]
async fn fresh_storage_inherits_nothing() {
    let charge = memory_limiter_consts::NEW_STORAGE_PAGE;
    let (mut storage, mem) = storage_with_budget(2, 2 * charge);

    storage
        .write_page(PageID(SlotID::ZERO, 0), [1; 32])
        .await
        .unwrap();
    storage
        .write_page(PageID(SlotID::ZERO, 1), [2; 32])
        .await
        .unwrap();

    assert_eq!(mem.get_remaining_memory(), 0);
}

// -- page id ordering ------------------------------------------------

#[test]
fn pages_sorted_correctly_1_byte() {
    let left = PageID(SlotID::from_bytes([1u8; 32]), 5);
    let right = PageID(SlotID::from_bytes([1u8; 32]), 10);
    assert!(left < right);
    assert!(left.to_bytes() < right.to_bytes());

    assert!(right > left);
    assert!(right.to_bytes() > left.to_bytes());
}

#[test]
fn pages_sorted_correctly_2_byte() {
    let left = PageID(SlotID::from_bytes([1u8; 32]), 5);
    let right = PageID(SlotID::from_bytes([1u8; 32]), 1024);
    assert!(left < right);
    assert!(left.to_bytes() < right.to_bytes());

    assert!(right > left);
    assert!(right.to_bytes() > left.to_bytes());
}
