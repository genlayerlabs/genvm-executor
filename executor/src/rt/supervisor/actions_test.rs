use super::*;
use std::collections::BTreeMap;

fn hash(n: u8) -> Bytes32Hash {
    Bytes32Hash::from_bytes([n; 32])
}

fn custom_id(n: u8) -> symbol_table::GlobalSymbol {
    runners::Id::Custom { hash: hash(n) }.canonical()
}

fn custom_id_str(n: u8) -> String {
    custom_id(n).as_str().to_owned()
}

fn pin(id: symbol_table::GlobalSymbol) -> runners::cache::ArchivePin {
    let arch = runners::Archive {
        data: BTreeMap::new(),
        total_size: 1,
    };
    let cell = std::sync::Arc::new(tokio::sync::OnceCell::new_with(Some(
        runners::ArchiveCache::new(id, arch),
    )));
    runners::cache::pin_of(cell)
}

/// A loaded set holding `custom:` entries for each of `hashes`.
fn parent_of(hashes: &[u8]) -> runners::cache::LoadedSet {
    let mut set = runners::cache::LoadedSet::default();
    for &n in hashes {
        set.insert(pin(custom_id(n)));
    }
    set
}

fn granted_ids(grants: &[runners::cache::ArchivePin]) -> Vec<String> {
    let mut ids: Vec<String> = grants
        .iter()
        .map(|p| p.runner_id().as_str().to_owned())
        .collect();
    ids.sort();
    ids
}

fn builtin_target() -> runners::Id {
    runners::Id::Builtin {
        name: symbol_table::GlobalSymbol::from("py"),
        hash: hash(200),
    }
}

fn contexts(items: &[&str]) -> VecDeque<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn mapping_target_cannot_hide_vm_behind_dot_component() {
    let target = "/./vm/secret";

    assert!(
        check_mapping_target(target).is_err(),
        "MapFile destination {target:?} normalizes into the protected /vm tree and must be rejected"
    );
}

#[test]
fn bounded_contexts_collapse_middle_at_limit() {
    let mut got = VecDeque::new();
    for idx in 0..16 {
        got = next_action_context(
            genvm_common::debug_mode::Capture::Bounded,
            &got,
            format!("ctx-{idx}"),
        );
    }

    assert_eq!(got.len(), 16);
    assert_eq!(got[0], "ctx-0");
    assert_eq!(got[1], "...");
    assert_eq!(got[2], "ctx-2");
    assert_eq!(got[15], "ctx-15");
}

#[test]
fn bounded_contexts_drop_after_existing_ellipsis() {
    let got = next_action_context(
        genvm_common::debug_mode::Capture::Bounded,
        &contexts(&[
            "ctx-0", "...", "ctx-2", "ctx-3", "ctx-4", "ctx-5", "ctx-6", "ctx-7", "ctx-8", "ctx-9",
            "ctx-10", "ctx-11", "ctx-12", "ctx-13", "ctx-14", "ctx-15",
        ]),
        "ctx-16".to_owned(),
    );

    assert_eq!(got.len(), 16);
    assert_eq!(got[0], "ctx-0");
    assert_eq!(got[1], "...");
    assert_eq!(got[2], "ctx-3");
    assert_eq!(got[15], "ctx-16");
}

#[test]
fn unbounded_contexts_do_not_collapse() {
    let mut got = VecDeque::new();
    for idx in 0..17 {
        got = next_action_context(
            genvm_common::debug_mode::Capture::Unbounded,
            &got,
            format!("ctx-{idx}"),
        );
    }

    assert_eq!(got.len(), 17);
    assert_eq!(got[1], "ctx-1");
    assert_eq!(got[16], "ctx-16");
}

#[test]
fn none_grants_the_whole_parent_custom_set() {
    let parent = parent_of(&[1, 2]);
    let got = resolve_child_custom_runners(&parent, None, &builtin_target()).unwrap();
    assert_eq!(
        granted_ids(&got),
        vec![custom_id_str(1), custom_id_str(2)],
        "should grant both parent custom entries"
    );
}

#[test]
fn some_list_grants_exactly_that_subset() {
    let parent = parent_of(&[1, 2]);
    let got =
        resolve_child_custom_runners(&parent, Some(vec![custom_id_str(1)]), &builtin_target())
            .unwrap();
    assert_eq!(granted_ids(&got), vec![custom_id_str(1)]);
}

#[test]
fn duplicate_element_is_rejected() {
    let parent = parent_of(&[1, 2]);
    let err = resolve_child_custom_runners(
        &parent,
        Some(vec![custom_id_str(1), custom_id_str(1)]),
        &builtin_target(),
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("duplicated"),
        "unexpected error: {err}"
    );
}

#[test]
fn non_custom_element_is_rejected() {
    let parent = parent_of(&[1]);
    let err = resolve_child_custom_runners(
        &parent,
        Some(vec!["py:abcdef".to_owned()]),
        &builtin_target(),
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("not a `custom:`"),
        "unexpected error: {err}"
    );
}

#[test]
fn element_outside_parent_set_is_rejected() {
    let parent = parent_of(&[1]);
    let err =
        resolve_child_custom_runners(&parent, Some(vec![custom_id_str(9)]), &builtin_target())
            .unwrap_err();
    assert!(
        err.to_string().contains("not loaded"),
        "unexpected error: {err}"
    );
}

#[test]
fn custom_target_loaded_in_parent_is_auto_included() {
    let parent = parent_of(&[1, 2]);
    // Empty explicit list, but the runner to execute is custom:1 (loaded).
    let target = runners::Id::Custom { hash: hash(1) };
    let got = resolve_child_custom_runners(&parent, Some(vec![]), &target).unwrap();
    assert_eq!(
        granted_ids(&got),
        vec![custom_id_str(1)],
        "target must be auto-granted"
    );
}

#[test]
fn custom_target_not_loaded_in_parent_is_rejected() {
    let parent = parent_of(&[1]);
    let target = runners::Id::Custom { hash: hash(9) };
    let err = resolve_child_custom_runners(&parent, None, &target).unwrap_err();
    assert!(
        err.to_string().contains("not loaded"),
        "unexpected error: {err}"
    );
}

#[test]
fn custom_target_already_granted_is_not_duplicated() {
    let parent = parent_of(&[1, 2]);
    let target = runners::Id::Custom { hash: hash(1) };
    // custom:1 appears both in the explicit grant list and as the target.
    let got = resolve_child_custom_runners(&parent, Some(vec![custom_id_str(1)]), &target).unwrap();
    assert_eq!(
        granted_ids(&got),
        vec![custom_id_str(1)],
        "no dup for target"
    );
}

// -- load-action charging --------------------------------------------

fn limiter_with_budget(budget: u32) -> rt::memlimiter::Limiter {
    let limiter = rt::memlimiter::Limiter::new();
    assert!(limiter.consume(u32::MAX - budget));
    limiter
}

fn fingerprint_of(fp: &sha3::Sha3_256) -> [u8; 32] {
    use sha3::Digest as _;
    fp.clone().finalize().into()
}

#[test]
fn charge_load_consumes_runner_load_cost_plus_size() {
    let limiter = limiter_with_budget(memory_limiter_consts::RUNNER_LOAD_COST + 100);
    charge_load(&limiter, 100).unwrap();
    assert_eq!(
        limiter.get_remaining_memory(),
        0,
        "charge must be exactly RUNNER_LOAD_COST + size"
    );
}

#[test]
fn charge_load_oom_charges_nothing() {
    let budget = memory_limiter_consts::RUNNER_LOAD_COST + 99;
    let limiter = limiter_with_budget(budget);
    let err = charge_load(&limiter, 100).unwrap_err();
    assert!(
        err.to_string().contains("out_of memory"),
        "unexpected error: {err}"
    );
    assert_eq!(
        limiter.get_remaining_memory(),
        budget,
        "a failed charge must leave the budget untouched"
    );
}

#[test]
fn charge_load_size_overflow_is_oom() {
    // RUNNER_LOAD_COST + u32::MAX overflows; must map to OOM, not wrap.
    let limiter = rt::memlimiter::Limiter::new();
    let err = charge_load(&limiter, u32::MAX as usize).unwrap_err();
    assert!(
        err.to_string().contains("out_of memory"),
        "unexpected error: {err}"
    );
    assert_eq!(limiter.get_remaining_memory(), u32::MAX);
}

// -- inherit load (grant transport) ----------------------------------

#[test]
fn inherit_load_charges_once_then_is_free() {
    // Grant pins have total_size 1 (see `pin`).
    let budget = 2 * (memory_limiter_consts::RUNNER_LOAD_COST + 1);
    let limiter = limiter_with_budget(budget);
    let mut loaded = runners::cache::LoadedSet::default();
    let granted = pin(custom_id(1));

    inherit_load(&limiter, &mut loaded, None, granted.clone()).unwrap();
    let after_first = limiter.get_remaining_memory();
    assert_eq!(
        budget - after_first,
        memory_limiter_consts::RUNNER_LOAD_COST + 1
    );
    assert!(loaded.contains(custom_id(1)), "grant must be pinned");

    // Same id again (e.g. also the child's custom entry point): free.
    inherit_load(&limiter, &mut loaded, None, granted).unwrap();
    assert_eq!(
        limiter.get_remaining_memory(),
        after_first,
        "an already-loaded runner must not be charged again"
    );
}

#[test]
fn inherit_load_oom_leaves_loaded_set_unchanged() {
    // One short of RUNNER_LOAD_COST + total_size(=1).
    let budget = memory_limiter_consts::RUNNER_LOAD_COST;
    let limiter = limiter_with_budget(budget);
    let mut loaded = runners::cache::LoadedSet::default();

    let err = inherit_load(&limiter, &mut loaded, None, pin(custom_id(1))).unwrap_err();
    assert!(
        err.to_string().contains("out_of memory"),
        "unexpected error: {err}"
    );
    assert!(!loaded.contains(custom_id(1)));
    assert_eq!(limiter.get_remaining_memory(), budget);
}

// -- det fingerprint -------------------------------------------------

#[test]
fn det_fingerprint_folds_charged_loads_in_execution_order() {
    let load = |ids: &[u8]| {
        let limiter = rt::memlimiter::Limiter::new();
        let mut loaded = runners::cache::LoadedSet::default();
        let mut fp = sha3::Sha3_256::default();
        for &n in ids {
            inherit_load(&limiter, &mut loaded, Some(&mut fp), pin(custom_id(n))).unwrap();
        }
        fingerprint_of(&fp)
    };

    assert_ne!(load(&[1]), load(&[2]), "different runner sets must diverge");
    assert_ne!(load(&[1, 2]), load(&[2, 1]), "order is part of the stream");
    assert_eq!(
        load(&[1, 2]),
        load(&[1, 2]),
        "same history, same fingerprint"
    );
}

#[test]
fn det_fingerprint_ignores_cached_loads() {
    let limiter = rt::memlimiter::Limiter::new();
    let mut loaded = runners::cache::LoadedSet::default();
    let mut fp = sha3::Sha3_256::default();

    inherit_load(&limiter, &mut loaded, Some(&mut fp), pin(custom_id(1))).unwrap();
    let after_charged = fingerprint_of(&fp);

    // A free (already-loaded) load must not alter the fingerprint.
    inherit_load(&limiter, &mut loaded, Some(&mut fp), pin(custom_id(1))).unwrap();
    assert_eq!(fingerprint_of(&fp), after_charged);
}

// -- RegisterRunner error ladder -------------------------------------

fn valid_code() -> bytes::Bytes {
    bytes::Bytes::from_static(b"# { \"Depends\": \"py-genlayer:test\" }\n")
}

fn custom_id_of(code: &bytes::Bytes) -> symbol_table::GlobalSymbol {
    runners::Id::Custom {
        hash: runners::custom_runner_hash(code),
    }
    .canonical()
}

#[tokio::test]
async fn register_charges_runner_load_cost_plus_code_len_and_pins() {
    let registry = runners::cache::WeakCache::new();
    let code = valid_code();
    let budget = 2 * (memory_limiter_consts::RUNNER_LOAD_COST + code.len() as u32);
    let limiter = limiter_with_budget(budget);
    let mut loaded = runners::cache::LoadedSet::default();

    let id = register_runner_load_into(&registry, &limiter, &mut loaded, None, code.clone())
        .await
        .unwrap();

    assert_eq!(id, custom_id_of(&code));
    assert_eq!(
        budget - limiter.get_remaining_memory(),
        memory_limiter_consts::RUNNER_LOAD_COST + code.len() as u32
    );
    assert!(loaded.contains(id), "registered runner must be resolvable");
}

#[tokio::test]
async fn register_same_code_in_same_vm_is_free() {
    let registry = runners::cache::WeakCache::new();
    let code = valid_code();
    let limiter = rt::memlimiter::Limiter::new();
    let mut loaded = runners::cache::LoadedSet::default();

    let id = register_runner_load_into(&registry, &limiter, &mut loaded, None, code.clone())
        .await
        .unwrap();
    let after_first = limiter.get_remaining_memory();

    for _ in 0..3 {
        let again = register_runner_load_into(&registry, &limiter, &mut loaded, None, code.clone())
            .await
            .unwrap();
        assert_eq!(again, id, "re-register must return the same id");
    }
    assert_eq!(
        limiter.get_remaining_memory(),
        after_first,
        "same-VM re-register must be free"
    );
}

#[tokio::test]
async fn register_oom_charges_and_registers_nothing() {
    let registry = runners::cache::WeakCache::new();
    let code = valid_code();
    let budget = memory_limiter_consts::RUNNER_LOAD_COST + code.len() as u32 - 1;
    let limiter = limiter_with_budget(budget);
    let mut loaded = runners::cache::LoadedSet::default();

    let err = register_runner_load_into(&registry, &limiter, &mut loaded, None, code.clone())
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("out_of memory"),
        "unexpected error: {err}"
    );
    assert_eq!(limiter.get_remaining_memory(), budget, "nothing charged");
    assert!(!loaded.contains(custom_id_of(&code)), "nothing pinned");
    assert!(
        !registry.cell(custom_id_of(&code)).initialized(),
        "nothing registered"
    );
}

#[tokio::test]
async fn register_parse_failure_retains_charge_and_is_not_resolvable() {
    let registry = runners::cache::WeakCache::new();
    // Not a zip, not wasm, not UTF-8 text: parse fails on the bytes alone.
    let code = bytes::Bytes::from_static(b"\xff\xfe\xfd");
    let budget = memory_limiter_consts::RUNNER_LOAD_COST + code.len() as u32;
    let limiter = limiter_with_budget(budget);
    let mut loaded = runners::cache::LoadedSet::default();

    let err = register_runner_load_into(&registry, &limiter, &mut loaded, None, code.clone())
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("invalid_contract"),
        "unexpected error: {err}"
    );
    assert_eq!(
        limiter.get_remaining_memory(),
        0,
        "the pre-parse charge is retained on parse failure"
    );
    assert!(
        !loaded.contains(custom_id_of(&code)),
        "a failed registration must not be resolvable"
    );
    assert!(
        !registry.cell(custom_id_of(&code)).initialized(),
        "malformed code must not enter the registry"
    );
}

/// Grant transport: the pins handed to a child at `RunNondet`/
/// `Sandbox` call time keep the content alive even after the granting parent
/// dies -- a queued nondet validator task must still find it and load it into
/// its own set, charged to its own limiter.
#[tokio::test]
async fn granted_pins_keep_content_alive_after_parent_death() {
    let registry = runners::cache::WeakCache::new();
    let code = valid_code();
    let parent_limiter = rt::memlimiter::Limiter::new();
    let mut parent = runners::cache::LoadedSet::default();
    let id = register_runner_load_into(&registry, &parent_limiter, &mut parent, None, code.clone())
        .await
        .unwrap();

    // gl_call time: the grant is computed and pinned while the parent lives.
    let grants = resolve_child_custom_runners(&parent, None, &builtin_target()).unwrap();

    // The parent VM dies before the queued child runs.
    drop(parent);
    assert!(
        registry.cell(id).initialized(),
        "granted pin must keep the content resident past the parent's death"
    );

    // Child spawn: inherit load actions charge the child's own limiter.
    let cost = memory_limiter_consts::RUNNER_LOAD_COST + code.len() as u32;
    let child_limiter = limiter_with_budget(cost);
    let mut child = runners::cache::LoadedSet::default();
    for grant in grants {
        inherit_load(&child_limiter, &mut child, None, grant).unwrap();
    }
    assert_eq!(
        child_limiter.get_remaining_memory(),
        0,
        "child pays for the grant"
    );
    assert!(child.contains(id));
}

#[tokio::test]
async fn register_dead_content_reparses_and_recharges_identically() {
    let registry = runners::cache::WeakCache::new();
    let code = valid_code();
    let cost = memory_limiter_consts::RUNNER_LOAD_COST + code.len() as u32;
    let limiter = limiter_with_budget(2 * cost);

    let mut loaded = runners::cache::LoadedSet::default();
    let id = register_runner_load_into(&registry, &limiter, &mut loaded, None, code.clone())
        .await
        .unwrap();
    assert_eq!(limiter.get_remaining_memory(), cost);

    // The registering scope dies: its loaded set (the only pin) drops and the
    // weak registry entry becomes dead.
    drop(loaded);
    assert!(!registry.cell(id).initialized(), "content freed with scope");

    // Re-register in a fresh scope: re-parses and charges the same amount.
    let mut fresh = runners::cache::LoadedSet::default();
    let again = register_runner_load_into(&registry, &limiter, &mut fresh, None, code)
        .await
        .unwrap();
    assert_eq!(again, id);
    assert_eq!(limiter.get_remaining_memory(), 0, "identical re-charge");
    assert!(fresh.contains(id));
}
