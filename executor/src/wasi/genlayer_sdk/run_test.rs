use super::*;
use crate::wasi::genlayer_sdk::tests::EmissionTestContext;

fn proposal() -> bytes::Bytes {
    rt::vm::ContractOutcome::Return(calldata::Value::Null.into())
        .encode()
        .into_bytes()
}

async fn context_at_fork_budget(
    mode: rt::RunMode,
    remaining: u32,
    proposal: bytes::Bytes,
) -> EmissionTestContext {
    let mut test = EmissionTestContext::with_nondet_mode(mode, proposal);
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

async fn assert_fork_oom(mode: rt::RunMode, proposal: bytes::Bytes) {
    let fork_charge = 4 * memory_limiter_consts::STORAGE_PAGE_INHERITED;
    let mut test = context_at_fork_budget(mode, fork_charge - 1, proposal).await;
    let result = test
        .wasi()
        .run_nondet(bytes::Bytes::new(), bytes::Bytes::new(), None, None, false)
        .await;
    let error = result.expect_err(&format!(
        "{mode:?} must reject unaffordable inherited pages"
    ));
    let trap: wasmtime::Error = error.downcast().expect_err("expected a trap");
    assert!(
        trap.to_string().contains("out_of memory"),
        "{mode:?}: {trap}"
    );
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

#[tokio::test]
async fn nondet_storage_fork_memory_edge_matches_all_modes() {
    for mode in [
        rt::RunMode::Leader,
        rt::RunMode::Validator,
        rt::RunMode::Sync,
    ] {
        assert_fork_oom(mode, proposal()).await;
    }
}

#[tokio::test]
async fn nondet_storage_fork_oom_precedes_empty_or_rejected_proposal() {
    for proposal in [bytes::Bytes::new(), bytes::Bytes::from_static(&[0xff])] {
        for mode in [
            rt::RunMode::Leader,
            rt::RunMode::Validator,
            rt::RunMode::Sync,
        ] {
            assert_fork_oom(mode, proposal.clone()).await;
        }
    }
}

#[tokio::test]
async fn nondet_storage_fork_exact_budget_uses_pre_output_snapshot() {
    let fork_charge = 4 * memory_limiter_consts::STORAGE_PAGE_INHERITED;
    for mode in [rt::RunMode::Validator, rt::RunMode::Sync] {
        let mut test = context_at_fork_budget(mode, fork_charge, proposal()).await;
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
        rt::supervisor::await_nondet_vms(&test.context.data.supervisor)
            .await
            .unwrap();
    }
}
