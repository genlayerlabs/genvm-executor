use genvm::calldata;
use genvm::config::{FeesBucketConfig, FeesConfig};
use genvm::public_abi::root_offsets;
use genvm::rt;
use genvm::rt::vm::storage::{
    default_code_slot, HostStorage, HostStorageLocking, Limiter, Storage,
};
use genvm::SlotID;
use genvm_common::sync;

// -- code slot reads (runner load action support) --------------------

/// In-memory host: one slot whose content is `code_len_le ++ code`; reads
/// beyond the content are zero-filled (like unwritten storage).
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

fn code_storage(code: &[u8]) -> Storage<FakeHost> {
    let mut slot = (code.len() as u32).to_le_bytes().to_vec();
    slot.extend_from_slice(code);
    Storage::new(
        calldata::Address::zero(),
        data_fees(u64::MAX),
        rt::memlimiter::Limiter::new(),
        FakeHost(std::sync::Arc::new(slot)),
    )
}

/// The runner load action reads the 4-byte length prefix first (to know
/// what to charge), then the blob at offset 4 -- the two reads must
/// reassemble exactly the stored code.
#[tokio::test]
async fn code_len_then_blob_reassembles_the_code() {
    // A length that is not 32-aligned exercises the page-offset math.
    let code: Vec<u8> = (0u16..77).map(|i| i as u8).collect();
    let mut storage = code_storage(&code);
    let slot = default_code_slot();

    let len = storage.read_code_len(slot).await.unwrap();
    assert_eq!(len, 77);

    let blob = storage.read_code_blob(slot, len).await.unwrap();
    assert_eq!(&blob[..], &code[..], "blob must skip the 4-byte prefix");
}

#[tokio::test]
async fn code_len_of_empty_code_is_zero() {
    let mut storage = code_storage(&[]);
    let slot = default_code_slot();
    assert_eq!(storage.read_code_len(slot).await.unwrap(), 0);
    assert!(storage.read_code_blob(slot, 0).await.unwrap().is_empty());
}

#[tokio::test]
async fn advisory_major_is_returned_with_resolved_code_slot() {
    let expected_slot = SlotID::from_bytes([0x5a; 32]);
    let mut root = vec![0; (root_offsets::CODE_SLOT + SlotID::SIZE) as usize];
    root[root_offsets::MAJOR as usize] = 2;
    root[root_offsets::CODE_SLOT as usize..].copy_from_slice(&expected_slot.raw());
    let mut storage = Storage::new(
        calldata::Address::zero(),
        data_fees(u64::MAX),
        rt::memlimiter::Limiter::new(),
        FakeHost(std::sync::Arc::new(root)),
    );

    let (major, slot) = storage.read_major_and_resolve_code_slot().await.unwrap();
    assert_eq!(major, 2);
    assert_eq!(slot, expected_slot);
}
