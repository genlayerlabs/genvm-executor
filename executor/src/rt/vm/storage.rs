use std::mem::MaybeUninit;
use std::ops::DerefMut;

use const_lru::ConstLru;
use genlayer_sdk::abi;
use genvm_common::{calldata, internal_constants::memory_limiter_consts, sync};

use crate::int_traits::*;
use crate::{public_abi::root_offsets, rt, SlotID};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(C)]
pub struct PageID(pub SlotID, pub u32);

impl PageID {
    pub fn to_bytes(&self) -> [u8; 36] {
        let mut res = [0u8; 36];
        res[..32].copy_from_slice(&self.0.raw());
        res[32..].copy_from_slice(&self.1.to_be_bytes());
        res
    }

    pub fn from_bytes(data: [u8; 36]) -> Self {
        let mut slot_bytes = [0u8; 32];
        slot_bytes.copy_from_slice(&data[..32]);
        let slot_id = SlotID::from_bytes(slot_bytes);
        let mut index_bytes = [0u8; 4];
        index_bytes.copy_from_slice(&data[32..]);
        let index = u32::from_be_bytes(index_bytes);
        Self(slot_id, index)
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Delta(
    #[serde(with = "serde_bytes")] [u8; 36],
    #[serde(with = "serde_bytes")] Vec<u8>,
);

impl Delta {
    pub(crate) fn cloned_parts(&self) -> ([u8; 36], Vec<u8>) {
        (self.0, self.1.clone())
    }

    #[cfg(test)]
    pub(crate) fn for_test(key: [u8; 36], value: Vec<u8>) -> Self {
        Self(key, value)
    }
}

impl<W: calldata::Writer> calldata::codec::Encode<W> for Delta {
    type Error = W::Error;

    fn encode(&self, enc: &mut calldata::Encoder<W>) -> Result<(), Self::Error> {
        enc.start_array(2)?;
        enc.push_bytes(&self.0)?;
        enc.push_bytes(&self.1)
    }
}

pub trait HostStorage {
    fn storage_read(&mut self, slot_id: SlotID, index: u32, buf: &mut [u8]) -> anyhow::Result<()>;
}

impl<HS: HostStorage, T: DerefMut<Target = HS>> HostStorage for T {
    fn storage_read(&mut self, slot_id: SlotID, index: u32, buf: &mut [u8]) -> anyhow::Result<()> {
        self.deref_mut().storage_read(slot_id, index, buf)
    }
}

pub trait HostStorageLocking {
    type ReturnType<'a>: HostStorage
    where
        Self: 'a;

    fn lock(&self) -> impl std::future::Future<Output = Self::ReturnType<'_>> + Send;
}

#[derive(Clone, Debug)]
pub struct Limiter {
    limit: sync::DArc<rt::fees::DataLimit>,
}

impl Limiter {
    pub fn new(data_fees_limit: sync::DArc<rt::fees::DataLimit>) -> Self {
        Self {
            limit: data_fees_limit,
        }
    }

    pub async fn consume(&self, amount: u64) -> rt::errors::Result<()> {
        if !self.limit.consume_storage_pages(amount).await? {
            return Err(rt::errors::Error::wrap(
                abi::consts::VmError::out_of().storage(),
                anyhow::anyhow!("consuming {amount} storage pages"),
            ));
        }
        Ok(())
    }
}

struct StoragePagesOverride {
    pages: rpds::RedBlackTreeMap<PageID, [u8; 32], archery::ArcTK>,
    fee: Limiter,
    mem: rt::memlimiter::Limiter,
    new_pages: u32,
}

impl StoragePagesOverride {
    fn new(storage_pages_limit: Limiter, mem: rt::memlimiter::Limiter) -> Self {
        Self {
            pages: Default::default(),
            fee: storage_pages_limit,
            mem,
            new_pages: 0,
        }
    }

    fn read_page_override(&self, key: PageID) -> Option<[u8; 32]> {
        self.pages.get(&key).cloned()
    }

    fn get(&self, key: PageID) -> Option<[u8; 32]> {
        self.pages.get(&key).cloned()
    }

    async fn write_page(&mut self, key: PageID, value: [u8; 32]) -> rt::errors::Result<()> {
        if !self.pages.contains_key(&key) {
            let new_pages = self.new_pages.checked_add(1).ok_or_else(|| {
                rt::errors::Error::wrap(
                    abi::consts::VmError::out_of().memory().val(),
                    anyhow::anyhow!("incrementing storage page count"),
                )
            })?;
            if !self.mem.consume(memory_limiter_consts::NEW_STORAGE_PAGE) {
                return Err(rt::errors::Error::wrap(
                    abi::consts::VmError::out_of().memory().val(),
                    anyhow::anyhow!("allocating storage page override"),
                ));
            }
            // Memory is charged first so a write refused for want of RAM costs no
            // fee; a write refused for want of fee must likewise cost no memory.
            if let Err(e) = self.fee.consume(1).await {
                self.mem.release(memory_limiter_consts::NEW_STORAGE_PAGE);
                return Err(e);
            }
            self.new_pages = new_pages;
        }
        self.pages = self.pages.insert(key, value);

        Ok(())
    }

    fn fork(&self, mem: rt::memlimiter::Limiter) -> rt::errors::Result<Self> {
        // While we keep our version alive the child can copy-on-write every page
        // it inherits, so it pays for them up front and gets it back when it ends.
        let inherited: u32 = self.pages.size().into_int_downcast().map_err(|_| {
            rt::errors::Error::wrap(
                abi::consts::VmError::out_of().memory().val(),
                anyhow::anyhow!("inheriting {} storage pages", self.pages.size()),
            )
        })?;
        if !mem.consume_mul(inherited, memory_limiter_consts::STORAGE_PAGE_INHERITED) {
            return Err(rt::errors::Error::wrap(
                abi::consts::VmError::out_of().memory().val(),
                anyhow::anyhow!("inheriting {inherited} storage pages"),
            ));
        }

        Ok(Self {
            pages: self.pages.clone(),
            fee: self.fee.clone(),
            mem,
            new_pages: 0,
        })
    }

    fn fold(&mut self, child: Self) -> rt::errors::Result<()> {
        // The parent pays only when it keeps the child's new pages.
        let new_pages = self.new_pages.checked_add(child.new_pages).ok_or_else(|| {
            rt::errors::Error::fatal_vm_cause(
                abi::consts::VmError::out_of().memory().val(),
                Some(anyhow::anyhow!("folding storage page count")),
            )
        })?;
        let charged = self
            .mem
            .consume_mul(child.new_pages, memory_limiter_consts::NEW_STORAGE_PAGE);
        // Unreachable: the child's budget is a snapshot of ours taken at spawn,
        // and it paid VM_SPAWN_COST out of it before writing a page, so what it
        // owes us is strictly less than what we still hold. Getting here means a
        // security researcher broke that invariant, so it is fatal and uncatchable.
        debug_assert!(charged, "storage fold charge exceeded the parent budget");
        if !charged {
            return Err(rt::errors::Error::fatal_vm_cause(
                abi::consts::VmError::out_of().memory().val(),
                Some(anyhow::anyhow!("folding storage page overrides")),
            ));
        }
        self.pages = child.pages;
        self.new_pages = new_pages;
        Ok(())
    }
}

const STORAGE_CACHE_SIZE: usize = 128;

pub struct Storage<HS: Send + Sync> {
    pub address: calldata::Address,
    host: HS,
    pages: StoragePagesOverride,
    cache: ConstLru<PageID, [u8; 32], STORAGE_CACHE_SIZE>,
}

impl<HS: Send + Sync> Storage<HS> {
    pub fn new(
        address: calldata::Address,
        storage_pages_limit: Limiter,
        mem: rt::memlimiter::Limiter,
        host: HS,
    ) -> Self {
        Self {
            address,
            host,
            pages: StoragePagesOverride::new(storage_pages_limit, mem),
            cache: ConstLru::new(),
        }
    }

    #[inline(always)]
    pub fn read_page_override(&self, key: PageID) -> Option<[u8; 32]> {
        self.pages.read_page_override(key)
    }

    #[inline(always)]
    pub async fn write_page(&mut self, key: PageID, value: [u8; 32]) -> rt::errors::Result<()> {
        self.pages.write_page(key, value).await
    }

    /// `mem` must be the very limiter the child VM runs on, otherwise its pages
    /// are charged to a budget nobody else draws from.
    pub fn fork(&self, mem: rt::memlimiter::Limiter) -> rt::errors::Result<Self>
    where
        HS: Clone,
    {
        Ok(Self {
            address: self.address,
            host: self.host.clone(),
            pages: self.pages.fork(mem)?,
            cache: self.cache.clone(),
        })
    }

    pub fn fold(&mut self, child: Self) -> rt::errors::Result<()> {
        self.pages.fold(child.pages)
    }

    pub fn pages_len(&self) -> usize {
        self.pages.pages.size()
    }

    pub fn make_delta(&self) -> Vec<Delta> {
        let mut res = Vec::<Delta>::new();

        for (k, v) in &self.pages.pages {
            if k.1 != 0 {
                let prev_page_id = PageID(k.0, k.1 - 1);
                if self.pages.pages.get(&prev_page_id).is_some() {
                    res.last_mut()
                        .expect("res must be non-empty when prev page exists")
                        .deref_mut()
                        .1
                        .extend_from_slice(v);
                    continue;
                }
            }
            res.push(Delta(k.to_bytes(), v.to_vec()));
        }

        res
    }
}

pub fn default_code_slot() -> SlotID {
    SlotID::ZERO.indirection(root_offsets::CODE)
}

impl<HS: HostStorageLocking + Send + Sync> Storage<HS> {
    pub async fn read(
        &mut self,
        slot_id: SlotID,
        index: u32,
        buf: &mut [u8],
    ) -> rt::errors::Result<()> {
        if buf.is_empty() {
            return Ok(());
        }

        let start_index = u32_into_usize(index);
        let end_index = start_index + buf.len();

        // Calculate page range
        let start_page = start_index / 32;
        let end_page = (end_index - 1) / 32;

        // Check which pages are known (in override or cache)
        let page_is_known = |this: &mut Self, page_id: PageID| -> bool {
            this.pages.get(page_id).is_some() || this.cache.get(&page_id).is_some()
        };

        // Multi-page case: cut known prefix and suffix
        let mut need_host_read_start = start_index;
        let mut need_host_read_end = end_index;

        // Cut known prefix
        for page_idx in start_page..=end_page {
            let page_id = PageID(slot_id, page_idx.into_int_downcast_panicking());
            if page_is_known(self, page_id) {
                need_host_read_start = (page_idx + 1) * 32;
            } else {
                break;
            }
        }

        // Cut known suffix
        for page_idx in (start_page..=end_page).rev() {
            let page_id = PageID(slot_id, page_idx.into_int_downcast_panicking());
            if page_is_known(self, page_id) {
                need_host_read_end = page_idx * 32;
            } else {
                break;
            }
        }

        // Read from host if needed
        if need_host_read_start < need_host_read_end {
            let host_read_len = need_host_read_end - need_host_read_start;
            let host_buf_offset = need_host_read_start - start_index;
            self.host.lock().await.storage_read(
                slot_id,
                need_host_read_start.into_int_downcast_panicking(),
                &mut buf[host_buf_offset..host_buf_offset + host_read_len],
            )?;
        }

        let mut put_to_cache: [MaybeUninit<(PageID, [u8; 32])>; STORAGE_CACHE_SIZE / 16] =
            // SAFETY: MaybeUninit does not require initialization
            unsafe { MaybeUninit::uninit().assume_init() };
        let mut put_to_cache_count = 0;

        for page_idx in start_page..=end_page {
            let page_id = PageID(slot_id, page_idx.into_int_downcast_panicking());

            let page_start_byte = page_idx * 32;
            let page_end_byte = page_start_byte + 32;
            if let Some(page_data) = self
                .pages
                .get(page_id)
                .or_else(|| self.cache.get(&page_id).cloned())
            {
                // Calculate overlap between requested range and this page
                let overlap_start = start_index.max(page_start_byte);
                let overlap_end = end_index.min(page_end_byte);

                if overlap_start < overlap_end {
                    let src_offset = overlap_start - page_start_byte;
                    let dst_offset = overlap_start - start_index;
                    let copy_len = overlap_end - overlap_start;

                    buf[dst_offset..dst_offset + copy_len]
                        .copy_from_slice(&page_data[src_offset..src_offset + copy_len]);
                }
            } else if page_start_byte >= need_host_read_start && page_end_byte <= need_host_read_end
            {
                // This page was read from host, extract it from buf and put to cache
                let src_offset = page_start_byte - start_index;
                let mut page_data = [0u8; 32];
                page_data.copy_from_slice(&buf[src_offset..src_offset + 32]);

                if put_to_cache_count < put_to_cache.len() {
                    put_to_cache[put_to_cache_count].write((page_id, page_data));
                    put_to_cache_count += 1;
                } else {
                    // staging buffer full: insert directly so no read page is dropped
                    self.cache.insert(page_id, page_data);
                }
            }
        }

        for item in put_to_cache.iter().take(put_to_cache_count) {
            // SAFETY: elements 0..put_to_cache_count have been initialized above
            let (page_id, page_data) = unsafe { item.assume_init_ref() };
            self.cache.insert(*page_id, *page_data);
        }

        Ok(())
    }

    async fn write_single_page(
        &mut self,
        page_id: PageID,
        offset_in_page: usize,
        buf: &[u8],
    ) -> rt::errors::Result<()> {
        let mut page_data = [0u8; 32];

        // Check if this is a full page write
        if offset_in_page == 0 && buf.len() == 32 {
            // Full page write
            page_data.copy_from_slice(buf);
        } else {
            // Partial page write - need existing data first
            if let Some(existing_page) = self.pages.get(page_id) {
                page_data.copy_from_slice(&existing_page);
            } else {
                // Read from host
                let page_start = (u32_into_usize(page_id.1) * 32).into_int_downcast_panicking();
                self.host
                    .lock()
                    .await
                    .storage_read(page_id.0, page_start, &mut page_data)?;
            }

            // Apply the write data
            page_data[offset_in_page..offset_in_page + buf.len()].copy_from_slice(buf);
        }

        self.write_page(page_id, page_data).await?;
        Ok(())
    }

    pub async fn write(
        &mut self,
        slot_id: SlotID,
        index: u32,
        buf: &[u8],
    ) -> rt::errors::Result<()> {
        if buf.is_empty() {
            return Ok(());
        }

        let start_index = u32_into_usize(index);
        let end_index = start_index + buf.len();

        // Calculate page range
        let start_page = start_index / 32;
        let end_page = (end_index - 1) / 32;

        // Handle single page case
        if start_page == end_page {
            let page_id = PageID(slot_id, start_page.into_int_downcast_panicking());
            let offset_in_page = start_index % 32;
            return self.write_single_page(page_id, offset_in_page, buf).await;
        }

        // Multi-page case

        let first_page_start = start_page * 32;
        let last_page_start = end_page * 32;

        let partial_first = start_index > first_page_start;
        let partial_last = end_index < last_page_start + 32;

        if partial_first || partial_last {
            let mut host_lock = self.host.lock().await;

            if partial_first {
                let page_id = PageID(slot_id, start_page.into_int_downcast_panicking());
                let mut page_data = [0u8; 32];

                if let Some(existing_page) = self.pages.get(page_id) {
                    page_data.copy_from_slice(&existing_page);
                } else {
                    host_lock.storage_read(
                        slot_id,
                        first_page_start.into_int_downcast_panicking(),
                        &mut page_data,
                    )?;
                }

                let offset_in_page = start_index % 32;
                let copy_len = 32 - offset_in_page;
                page_data[offset_in_page..].copy_from_slice(&buf[..copy_len]);

                self.pages.write_page(page_id, page_data).await?;
            }

            if partial_last {
                let page_id = PageID(slot_id, end_page.into_int_downcast_panicking());
                let mut page_data = [0u8; 32];

                if let Some(existing_page) = self.pages.get(page_id) {
                    page_data.copy_from_slice(&existing_page);
                } else {
                    host_lock.storage_read(
                        slot_id,
                        last_page_start.into_int_downcast_panicking(),
                        &mut page_data,
                    )?;
                }

                let end_offset_in_page = end_index % 32;
                let src_offset = buf.len() - end_offset_in_page;
                page_data[..end_offset_in_page].copy_from_slice(&buf[src_offset..]);

                self.pages.write_page(page_id, page_data).await?;
            }
        }

        // Write all middle pages directly
        for page_idx in start_page..=end_page {
            let page_start = page_idx * 32;
            let page_end = page_start + 32;

            // Skip if this is a partial page we already handled
            if (page_idx == start_page && start_index > page_start)
                || (page_idx == end_page && end_index < page_end)
            {
                continue;
            }

            let page_id = PageID(slot_id, page_idx.into_int_downcast_panicking());
            let src_offset = page_start - start_index;
            let mut page_data = [0u8; 32];
            page_data.copy_from_slice(&buf[src_offset..src_offset + 32]);
            self.write_page(page_id, page_data).await?;
        }

        Ok(())
    }

    pub async fn write_code(&mut self, code: &[u8]) -> rt::errors::Result<()> {
        let code_slot = default_code_slot();

        if code.len() > u32_into_usize(u32::MAX - 4) {
            return Err(rt::errors::Error::vm(
                abi::consts::VmError::out_of().storage(),
            ));
        }

        let code_len: u32 = code.len().into_int_downcast_panicking();
        let mut len_buf = [0; 4];
        len_buf.copy_from_slice(&code_len.to_le_bytes());
        self.write(code_slot, 0, &len_buf).await?;
        self.write(code_slot, 4, code).await?;

        Ok(())
    }

    /// Reads the GenVM major version the contract was deployed for, from the
    /// `major` root field (zero if never written).
    pub async fn read_major(&mut self) -> rt::errors::Result<u8> {
        let mut buf = [0u8; 1];
        self.read(SlotID::ZERO, root_offsets::MAJOR, &mut buf)
            .await?;
        Ok(buf[0])
    }

    /// Writes the GenVM major version the contract is deployed for into the
    /// `major` root field, so later runs can verify major compatibility (see
    /// [`read_major`]). Written on deploy alongside the code.
    pub async fn write_major(&mut self, major: u8) -> rt::errors::Result<()> {
        self.write(SlotID::ZERO, root_offsets::MAJOR, &[major])
            .await
    }

    /// Resolves the slot the contract code is stored at: the raw `code_slot` root
    /// field if set, otherwise the default `code` slot.
    pub async fn resolve_code_slot(&mut self) -> rt::errors::Result<SlotID> {
        let mut raw = [0u8; 32];
        self.read(SlotID::ZERO, root_offsets::CODE_SLOT, &mut raw)
            .await?;
        if raw == [0u8; 32] {
            Ok(default_code_slot())
        } else {
            Ok(SlotID::from_bytes(raw))
        }
    }

    /// Reads the contract's inline permission bitfield from the root slot as a
    /// raw 32-byte little-endian `u256`; bit `n` is the `Permissions` member with
    /// value `n`. Zero (never-written default) grants nothing.
    pub async fn read_permissions(&mut self) -> rt::errors::Result<[u8; 32]> {
        let mut raw = [0u8; 32];
        self.read(SlotID::ZERO, root_offsets::PERMISSIONS, &mut raw)
            .await?;
        Ok(raw)
    }

    /// Verifies the contract's major version matches this node's, then resolves
    /// its code slot. The major is checked *first*, before trusting the
    /// host-read code-slot pointer; on mismatch a `major_mismatch` [`Error`]
    /// is returned so the caller surfaces it as a contract error.
    pub async fn check_major_and_resolve_code_slot(&mut self) -> rt::errors::Result<SlotID> {
        let contract_major = self.read_major().await?;
        Self::check_major(contract_major)?;
        self.resolve_code_slot().await
    }

    pub fn check_major(contract_major: u8) -> rt::errors::Result<()> {
        let node_major = genvm_common::version::CURRENT.major;
        if u16::from(contract_major) != node_major {
            return Err(rt::errors::Error::wrap(
                abi::consts::VmError::invalid_contract().major_mismatch(),
                anyhow::anyhow!("contract major {contract_major} != node major {node_major}"),
            ));
        }
        Ok(())
    }

    /// Reads the advisory major and resolves the code slot without enforcing
    /// compatibility. Callers must either delegate the contract or perform the
    /// major check before using the returned slot in this executor.
    pub async fn read_major_and_resolve_code_slot(&mut self) -> rt::errors::Result<(u8, SlotID)> {
        let contract_major = self.read_major().await?;
        let code_slot = self.resolve_code_slot().await?;
        Ok((contract_major, code_slot))
    }

    /// Reads the 4-byte little-endian length prefix of the code blob at
    /// `code_slot`. Cheap (one slot read, no fee metering) -- the runner load
    /// action reads this first to learn the size to charge *before* fetching the
    /// blob, so a single `RUNNER_LOAD_COST + code_size` charge covers the peak.
    pub async fn read_code_len(&mut self, code_slot: SlotID) -> rt::errors::Result<u32> {
        let mut len_buf = [0; 4];
        self.read(code_slot, 0, &mut len_buf).await?;
        Ok(u32::from_le_bytes(len_buf))
    }

    /// Reads the code blob of `code_size` bytes at `code_slot` (after its 4-byte
    /// length prefix). Charges nothing: the load action already charged
    /// `code_size` via [`read_code_len`](Self::read_code_len).
    pub async fn read_code_blob(
        &mut self,
        code_slot: SlotID,
        code_size: u32,
    ) -> rt::errors::Result<Box<[u8]>> {
        let res = Box::new_uninit_slice(code_size.into_int_comptime());
        let mut res = unsafe { res.assume_init() };

        self.read(code_slot, 4, &mut res).await?;

        Ok(res)
    }
}
