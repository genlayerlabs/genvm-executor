use super::*;
use crate::wasi::genlayer_sdk::tests::EmissionTestContext;

fn proposal() -> bytes::Bytes {
    rt::vm::ContractOutcome::Return(calldata::Value::Null.into())
        .encode()
        .into_bytes()
}

async fn context_at_fork_budget(mode: rt::RunMode, remaining: u32) -> EmissionTestContext {
    let mut test = EmissionTestContext::with_nondet_mode(mode, proposal());
    test.context.data.conf.permissions.spawn_nondet = true;
    test.context.data.conf.execution.topmost_runner_id = runners::Id::Chain {
        address: calldata::Address::zero(),
        on: runners::ChainState::Decided,
        slot: SlotID::ZERO,
    };
    for page in 0..4 {
        test.context
            .data
            .storage
            .write_page(rt::vm::storage::PageID(SlotID::ZERO, page), [1; 32])
            .await
            .unwrap();
    }
    let limiter = &test.context.limiter;
    assert!(limiter.consume(limiter.get_remaining_memory() - remaining));
    test
}

#[tokio::test]
async fn nondet_storage_fork_memory_edge_matches_all_modes() {
    let fork_charge = 4 * memory_limiter_consts::STORAGE_PAGE_INHERITED;
    for remaining in [fork_charge - 1, fork_charge - 64] {
        for mode in [
            rt::RunMode::Leader,
            rt::RunMode::Validator,
            rt::RunMode::Sync,
        ] {
            let mut test = context_at_fork_budget(mode, remaining).await;
            let result = test
                .wasi()
                .run_nondet(bytes::Bytes::new(), bytes::Bytes::new(), None, None, false)
                .await;
            let error = result.expect_err(&format!(
                "{mode:?} must reject unaffordable inherited pages"
            ));
            let trap: wasmtime::Error = error.downcast().expect_err("expected a trap");
            assert!(
                trap.to_string().contains("inheriting 4 storage pages"),
                "{mode:?}: {trap}"
            );
            assert_eq!(
                test.context
                    .data
                    .supervisor
                    .nondet_call_no
                    .load(std::sync::atomic::Ordering::SeqCst),
                1
            );
        }
    }
}

#[tokio::test]
async fn nondet_storage_fork_sync_exact_budget_uses_pre_output_snapshot() {
    let fork_charge = 4 * memory_limiter_consts::STORAGE_PAGE_INHERITED;
    let mut test = context_at_fork_budget(rt::RunMode::Sync, fork_charge).await;
    test.wasi()
        .run_nondet(bytes::Bytes::new(), bytes::Bytes::new(), None, None, false)
        .await
        .unwrap();
    assert_eq!(
        test.context
            .data
            .supervisor
            .nondet_call_no
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert!(test.context.limiter.get_remaining_memory() > 0);
}
