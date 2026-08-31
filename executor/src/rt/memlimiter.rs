use genvm_common::*;
use std::sync::{atomic::AtomicU32, Arc};

use crate::int_traits::*;
use crate::rt;

struct LimiterInner {
    remaining_memory: AtomicU32,
    new_permanent_allocations: AtomicU32,
}

#[derive(Clone)]
pub struct Limiter(Arc<LimiterInner>);

impl LimiterInner {
    fn release_no_consumed(&self, delta: u32) {
        #[allow(dead_code)]
        let previous = self
            .remaining_memory
            .fetch_add(delta, std::sync::atomic::Ordering::SeqCst);
        assert!(previous.checked_add(delta).is_some());
    }
}

impl Default for Limiter {
    fn default() -> Self {
        Self::new()
    }
}

impl Limiter {
    pub fn new() -> Self {
        Self::with_limit(u32::MAX)
    }

    pub fn with_limit(limit: u32) -> Self {
        Self(Arc::new(LimiterInner {
            remaining_memory: AtomicU32::new(limit),
            new_permanent_allocations: AtomicU32::new(0),
        }))
    }

    pub fn derived(&self) -> Self {
        Self(Arc::new(LimiterInner {
            remaining_memory: AtomicU32::new(
                self.0
                    .remaining_memory
                    .load(std::sync::atomic::Ordering::SeqCst),
            ),
            new_permanent_allocations: AtomicU32::new(0),
        }))
    }

    pub fn reserve_permanent(&self, delta: u64) -> Option<PermanentAllocation> {
        let delta = u32::try_from(delta).ok()?;
        if !self.consume(delta) {
            return None;
        }
        if self
            .0
            .new_permanent_allocations
            .fetch_update(
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
                |current| current.checked_add(delta),
            )
            .is_err()
        {
            self.release(delta);
            return None;
        }
        Some(PermanentAllocation {
            limiter: self.clone(),
            delta,
            committed: false,
        })
    }

    pub fn fold_permanent(&self, child: &Self) -> bool {
        let delta = child
            .0
            .new_permanent_allocations
            .load(std::sync::atomic::Ordering::SeqCst);
        if self
            .0
            .new_permanent_allocations
            .fetch_update(
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
                |current| current.checked_add(delta),
            )
            .is_err()
        {
            return false;
        }
        if !self.consume(delta) {
            return false;
        }
        true
    }

    pub fn get_new_permanent_allocations(&self) -> u32 {
        self.0
            .new_permanent_allocations
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Charges `delta` bytes, failing (rather than truncating) when `delta`
    /// does not fit in the `u32` budget. A body larger than `u32::MAX` can never
    /// fit the 4 GiB budget anyway, so this maps cleanly onto the OOM path.
    pub fn consume_size(&self, delta: usize) -> bool {
        match u32::try_from(delta) {
            Ok(delta) => self.consume(delta),
            Err(_) => false,
        }
    }

    pub fn consume_mul(&self, delta: u32, multiplier: u32) -> bool {
        let delta = match delta.checked_mul(multiplier) {
            Some(delta) => delta,
            None => return false,
        };

        self.consume(delta)
    }

    pub fn get_remaining_memory(&self) -> u32 {
        self.0
            .remaining_memory
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn release(&self, delta: u32) {
        self.0.release_no_consumed(delta);
    }

    pub fn consume(&self, delta: u32) -> bool {
        let mut remaining = self
            .0
            .remaining_memory
            .load(std::sync::atomic::Ordering::SeqCst);

        log_trace!(delta = delta, remaining_at_op_start = remaining; "consume");

        loop {
            if delta > remaining {
                return false;
            }

            match self.0.remaining_memory.compare_exchange(
                remaining,
                remaining - delta,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            ) {
                Ok(prev_remaining) => {
                    debug_assert!(
                        prev_remaining.checked_sub(delta).is_some(),
                        "limiter remaining_memory overflow: {prev_remaining} - {delta}",
                    );
                    break;
                }
                Err(new_remaining) => remaining = new_remaining,
            }
        }

        true
    }
}

/// A RAM charge released on drop unless [`PermanentAllocation::commit`] retains it.
pub struct PermanentAllocation {
    limiter: Limiter,
    delta: u32,
    committed: bool,
}

impl PermanentAllocation {
    pub fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for PermanentAllocation {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        self.limiter
            .0
            .new_permanent_allocations
            .fetch_sub(self.delta, std::sync::atomic::Ordering::SeqCst);
        self.limiter.release(self.delta);
    }
}

impl wasmtime::ResourceLimiter for Limiter {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let delta = desired - current;
        if delta > u32_into_usize(u32::MAX) {
            return Ok(false);
        }

        let delta = delta.into_int_downcast_panicking();
        let success = self.consume(delta);

        if current == 0 && !success {
            Err(rt::errors::Error::vm(
                genlayer_sdk::abi::consts::VmError::out_of()
                    .memory()
                    .wasm_memory(),
            )
            .into())
        } else {
            Ok(success)
        }
    }

    fn table_growing(
        &mut self,
        current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let delta = desired - current;

        if delta > u32_into_usize(u32::MAX) {
            return Ok(false);
        }

        let delta = delta.into_int_downcast_panicking();
        let success = self.consume_mul(
            delta,
            genvm_common::internal_constants::memory_limiter_consts::TABLE_ENTRY,
        );

        if current == 0 && !success {
            Err(rt::errors::Error::vm(
                genlayer_sdk::abi::consts::VmError::out_of()
                    .memory()
                    .wasm_table(),
            )
            .into())
        } else {
            Ok(success)
        }
    }

    fn instances(&self) -> usize {
        1000
    }

    fn tables(&self) -> usize {
        100
    }

    fn memories(&self) -> usize {
        100
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_spawn_limit_is_honored() {
        let limiter = Limiter::with_limit(100);
        assert_eq!(limiter.get_remaining_memory(), 100);
        assert!(limiter.consume(40));
        assert_eq!(limiter.get_remaining_memory(), 60);
        assert!(!limiter.consume(61));
    }
}
