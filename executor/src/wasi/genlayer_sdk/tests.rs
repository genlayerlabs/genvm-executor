use super::message::{
    external_allocation_candidates, next_message_is_first, resolve_internal_allocation,
    validate_balance_fee, FEE_PARAM_COUNT_BITS, FEE_PARAM_PRICE_BITS,
};
use super::run::{
    call_contract_route, charge_nondet_output, derive_call_contract_permissions,
    leader_outcome_for_publication, leader_proposal_for_validation, nested_run_ok,
    parse_leader_result, preflight_nondet_output_fees, preflight_nondet_output_ram,
    reserve_nondet_output, strip_vm_error_detail, validate_leader_output_after_caps,
    CallContractRoute, LeaderProposal, NondetOutput,
};
use super::*;
use genvm_common::Bytes32Hash;
use primitive_types::U256;

fn valid_params() -> abi::fees::InternalMessageParams {
    abi::fees::InternalMessageParams {
        leader_time_units_allocation: U256::from(5),
        validator_time_units_allocation: U256::from(5),
        execution_budget_per_round: U256::from(1024),
        rotations: vec![U256::from(4); 5],
        max_price_gen_per_time_unit: U256::from(2),
        storage_fee_max_gas_price: U256::from(20),
        receipt_fee_max_gas_price: U256::from(20),
    }
}

fn errno(e: generated::types::Error) -> generated::types::Errno {
    e.downcast().expect("expected a plain errno, got a trap")
}

fn nondet_fees_with_delta(total: u64, nondet_delta: &str) -> rt::fees::DataLimit {
    let bucket = |delta: &str| crate::config::FeesBucketConfig {
        buckets: vec![symbol_table::GlobalSymbol::from("test")],
        subtract_on_start_expr: "0".to_owned(),
        delta_expr: delta.to_owned(),
    };
    rt::fees::DataLimit::new(
        std::collections::HashMap::from([("test".to_owned(), U256::from(total))]),
        crate::config::FeesConfig {
            expr_prelude: String::new(),
            storage: bucket("\\attrs = 0"),
            message_receipt: bucket("\\attrs = 0"),
            nondet_output: bucket(nondet_delta),
            message_fee: bucket("\\attrs = 0"),
            event: bucket("\\attrs = 0"),
        },
        Default::default(),
    )
    .unwrap()
}

fn nondet_fees(total: u64) -> rt::fees::DataLimit {
    nondet_fees_with_delta(total, "\\attrs = attrs.outputLength")
}

fn emission_fees() -> crate::config::FeesConfig {
    let bucket = |delta: &str| crate::config::FeesBucketConfig {
        buckets: vec![symbol_table::GlobalSymbol::from("test")],
        subtract_on_start_expr: "0".to_owned(),
        delta_expr: delta.to_owned(),
    };
    crate::config::FeesConfig {
        expr_prelude: String::new(),
        storage: bucket("\\attrs = 0"),
        message_receipt: bucket("\\attrs = 1"),
        nondet_output: bucket("\\attrs = 0"),
        message_fee: bucket("\\attrs = 1"),
        event: bucket("\\attrs = 1"),
    }
}

fn external_message_allocation() -> genvm_modules_interfaces::fees::MessageAllocationNode {
    genvm_modules_interfaces::fees::MessageAllocationNode {
        recipient: None,
        call_key: None,
        budget: U256::from(100),
        on: genvm_modules_interfaces::On::Finalized,
        fee_params: genvm_modules_interfaces::fees::MessageAllocationNodeParams::External(
            genvm_modules_interfaces::fees::ExternalMessageParams {
                gas_limit: U256::zero(),
                max_gas_price: U256::zero(),
            },
        ),
        children: Vec::new(),
    }
}

fn internal_message_allocation() -> genvm_modules_interfaces::fees::MessageAllocationNode {
    genvm_modules_interfaces::fees::MessageAllocationNode {
        recipient: None,
        call_key: None,
        budget: U256::from(100),
        on: genvm_modules_interfaces::On::Finalized,
        fee_params: genvm_modules_interfaces::fees::MessageAllocationNodeParams::Internal(
            Arc::new(genvm_modules_interfaces::fees::InternalMessageParams {
                leader_timeunits_allocation: U256::one(),
                validator_timeunits_allocation: U256::one(),
                execution_budget_per_round: U256::one(),
                rotations: vec![U256::one()],
                max_price_gen_per_time_unit: U256::one(),
                storage_fee_max_gas_price: U256::one(),
                receipt_fee_max_gas_price: U256::one(),
            }),
        ),
        children: Vec::new(),
    }
}

fn allocation_child(
    budget: U256,
    children: Vec<genvm_modules_interfaces::fees::MessageAllocationNode>,
) -> genvm_modules_interfaces::fees::MessageAllocationNode {
    let mut node = internal_message_allocation();
    node.budget = budget;
    node.children = children;
    node
}

#[test]
fn internal_allocation_prefers_exact_key_over_earlier_wildcard() {
    let recipient = calldata::Address::from([7; 20]);
    let call_key = genvm_modules_interfaces::abi_stub::CallKey([8; 32]);
    let mut wildcard = internal_message_allocation();
    wildcard.recipient = Some(recipient);
    wildcard.budget = U256::one();
    let mut exact = wildcard.clone();
    exact.call_key = Some(call_key);
    exact.budget = U256::from(2);
    let nodes = vec![wildcard, exact];

    let (matched, _) = resolve_internal_allocation(
        &nodes,
        genvm_modules_interfaces::On::Finalized,
        recipient,
        call_key,
    )
    .expect("exact allocation should match");

    assert_eq!(nodes[matched].budget, U256::from(2));
}

#[test]
fn internal_allocation_skips_zero_budget_exact_key() {
    let recipient = calldata::Address::from([7; 20]);
    let call_key = genvm_modules_interfaces::abi_stub::CallKey([8; 32]);
    let mut wildcard = internal_message_allocation();
    wildcard.recipient = Some(recipient);
    wildcard.budget = U256::one();
    let mut exact = wildcard.clone();
    exact.call_key = Some(call_key);
    exact.budget = U256::zero();
    let nodes = vec![wildcard, exact];

    let (matched, _) = resolve_internal_allocation(
        &nodes,
        genvm_modules_interfaces::On::Finalized,
        recipient,
        call_key,
    )
    .expect("wildcard allocation should match");

    assert_eq!(nodes[matched].budget, U256::one());
}

#[test]
fn internal_allocation_phase_is_checked_after_key_resolution() {
    let recipient = calldata::Address::from([7; 20]);
    let call_key = genvm_modules_interfaces::abi_stub::CallKey([8; 32]);
    let mut wildcard = internal_message_allocation();
    wildcard.recipient = Some(recipient);
    let mut exact = wildcard.clone();
    exact.call_key = Some(call_key);
    exact.on = genvm_modules_interfaces::On::Decided;
    let nodes = vec![wildcard, exact];

    assert!(resolve_internal_allocation(
        &nodes,
        genvm_modules_interfaces::On::Finalized,
        recipient,
        call_key,
    )
    .is_none());
}

#[test]
fn internal_allocation_selects_phase_within_equal_keys() {
    let recipient = calldata::Address::from([7; 20]);
    let call_key = genvm_modules_interfaces::abi_stub::CallKey([8; 32]);
    let mut finalized = internal_message_allocation();
    finalized.budget = U256::one();
    let mut decided = finalized.clone();
    decided.on = genvm_modules_interfaces::On::Decided;
    decided.budget = U256::from(2);
    let nodes = vec![finalized, decided];

    let (matched, _) = resolve_internal_allocation(
        &nodes,
        genvm_modules_interfaces::On::Decided,
        recipient,
        call_key,
    )
    .expect("decided allocation should match");

    assert_eq!(nodes[matched].budget, U256::from(2));
}

#[test]
fn external_allocation_candidates_follow_consensus_precedence() {
    let recipient = calldata::Address::from([7; 20]);
    let call_key = genvm_modules_interfaces::abi_stub::CallKey([8; 32]);
    let mut global_wildcard = external_message_allocation();
    global_wildcard.budget = U256::one();
    let mut recipient_wildcard = global_wildcard.clone();
    recipient_wildcard.recipient = Some(recipient);
    recipient_wildcard.budget = U256::from(2);
    let mut exact = recipient_wildcard.clone();
    exact.call_key = Some(call_key);
    exact.budget = U256::from(3);
    let nodes = vec![global_wildcard, recipient_wildcard, exact];

    let candidates = external_allocation_candidates(&nodes, recipient, call_key);
    let budgets = candidates
        .into_iter()
        .map(|index| nodes[index].budget)
        .collect::<Vec<_>>();

    assert_eq!(budgets, vec![U256::from(3), U256::from(2), U256::one()]);
}

#[test]
fn external_allocation_candidates_include_zero_budget_nodes() {
    let recipient = calldata::Address::from([7; 20]);
    let call_key = genvm_modules_interfaces::abi_stub::CallKey([8; 32]);
    let mut node = external_message_allocation();
    node.recipient = Some(recipient);
    node.call_key = Some(call_key);
    node.budget = U256::zero();
    let nodes = vec![node];

    assert_eq!(
        external_allocation_candidates(&nodes, recipient, call_key),
        vec![0]
    );
}

struct TestDir(std::path::PathBuf);

impl TestDir {
    fn new() -> Self {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

        loop {
            let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let root = std::env::temp_dir()
                .join(format!("genvm-emission-test-{}-{id}", std::process::id()));
            match std::fs::create_dir(&root) {
                Ok(()) => return Self(root),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("creating emission test directory: {error}"),
            }
        }
    }
}

impl std::ops::Deref for TestDir {
    type Target = std::path::Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Copy)]
enum MessageEmission {
    External,
    InternalAllocation,
    InternalBalance,
    DeployAllocation,
    DeployBalance,
}

impl MessageEmission {
    const ALL: [Self; 5] = [
        Self::External,
        Self::InternalAllocation,
        Self::InternalBalance,
        Self::DeployAllocation,
        Self::DeployBalance,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::External => "external message",
            Self::InternalAllocation => "allocation-funded internal message",
            Self::InternalBalance => "balance-funded internal message",
            Self::DeployAllocation => "allocation-funded deploy message",
            Self::DeployBalance => "balance-funded deploy message",
        }
    }

    fn fee_error(self) -> &'static str {
        match self {
            Self::External => "out_of message_fee total # external",
            Self::InternalAllocation | Self::DeployAllocation => {
                "out_of message_fee total # internal"
            }
            Self::InternalBalance | Self::DeployBalance => "out_of receipt message # internal",
        }
    }
}

struct EmissionTestContext {
    _host_peer: std::os::unix::net::UnixStream,
    vfs: vfs::VFS,
    preview1: super::super::preview1::Context,
    context: Context,
    _root: TestDir,
}

impl EmissionTestContext {
    fn new(memory_limit: u32, fee_total: u64) -> Self {
        let root = TestDir::new();
        let runners_dir = root.join("runners");
        let registry_dir = root.join("registry");
        std::fs::create_dir_all(&runners_dir).unwrap();
        std::fs::create_dir_all(&registry_dir).unwrap();
        std::fs::write(registry_dir.join("all.json"), "{}").unwrap();

        let fees = emission_fees();
        let shared_data = sync::DArc::new(rt::SharedData {
            run_mode: rt::RunMode::Leader,
            genvm_id: genvm_modules_interfaces::GenVMId(0),
            debug_mode: genvm_common::DebugMode::Disabled,
            metrics: crate::Metrics {
                hosts: vec![Default::default()].into_boxed_slice(),
                ..Default::default()
            },
            data_fees_limit: rt::fees::DataLimit::new(
                std::collections::HashMap::from([("test".to_owned(), U256::from(fee_total))]),
                fees.clone(),
                Default::default(),
            )
            .unwrap(),
            det_fuel_budget: rt::DetFuelBudget::new(None),
            llm_consumption: tokio::sync::Mutex::new(U256::zero()),
        });
        let host_data = genvm_modules_interfaces::HostData {
            node_address: String::new(),
            tx_id: String::new(),
            rest: Default::default(),
        };
        let module = |name: &str, metrics| {
            Arc::new(crate::modules::Module::new(
                crate::modules::ModuleNamedArgs {
                    name: name.to_owned(),
                    url: "127.0.0.1:1".to_owned(),
                    gas_data: Default::default(),
                    initial_time_units_allocation: 0,
                },
                genvm_modules_interfaces::GenVMId(0),
                genvm_modules_interfaces::Role::Leader,
                host_data.clone(),
                metrics,
            ))
        };
        let modules = crate::modules::All {
            web: module("web", shared_data.gep(|data| &data.metrics.web_module)),
            llm: module("llm", shared_data.gep(|data| &data.metrics.llm_module)),
        };
        let (host_stream, host_peer) = std::os::unix::net::UnixStream::pair().unwrap();
        let host = host::Host::new(
            Box::new(bufreaderwriter::seq::BufReaderWriterSeq::new_writer(
                host_stream,
            )),
            shared_data.gep(|data| &data.metrics.hosts[0]),
        );
        let config = crate::config::Config {
            modules: crate::config::Modules {
                llm: crate::config::Module {
                    address: "127.0.0.1:1".to_owned(),
                },
                web: crate::config::Module {
                    address: "127.0.0.1:1".to_owned(),
                },
            },
            fees,
            cache_dir: root.join("cache").to_string_lossy().into_owned(),
            runners_dir: runners_dir.to_string_lossy().into_owned(),
            registry_dir: registry_dir.to_string_lossy().into_owned(),
            base: genvm_common::BaseConfig {
                threads: 1,
                blocking_threads: 1,
                log_level: genvm_common::logger::Level::Info,
                log_disable: String::new(),
            },
        };
        let supervisor = rt::supervisor::Supervisor::start(
            &config,
            rt::supervisor::Ctor {
                shared_data: shared_data.clone(),
                modules,
                locked_slots: host::LockedSlotsSet::empty(),
                leader_nondet_results: None,
                emit_leader_public_data: false,
                multi_host: host::MultiHost::new(vec![host], Vec::new()),
                record_actions: Vec::new(),
            },
        )
        .unwrap();
        supervisor
            .balances
            .insert(calldata::Address::zero(), U256::MAX);

        let limiter = rt::memlimiter::Limiter::with_limit(memory_limit);
        let permissions = base::Permissions {
            deterministic: true,
            write_storage: true,
            send_messages: true,
            call_others: false,
            spawn_nondet: false,
            can_use_balance_for_message_fees: true,
        };
        let conf = base::Config {
            needs_error_fingerprint: false,
            permissions,
            execution: base::Execution {
                state_mode: public_abi::StorageView::Default,
                topmost_runner_id: crate::runners::Id::Custom {
                    hash: Bytes32Hash::ZERO,
                },
            },
        };
        let address = calldata::Address::zero();
        let storage = rt::vm::storage::Storage::new(
            address,
            supervisor.get_storage_limiter(),
            limiter.clone(),
            StorageHostHolder(
                supervisor.host.clone(),
                ReadToken {
                    mode: public_abi::StorageView::Default,
                    account: address,
                },
            ),
        );
        let message_data = ExtendedMessage {
            message: abi::entry::MessageData {
                contract_address: address,
                sender_address: address,
                origin_address: address,
                signer_address: address,
                stack: Vec::new(),
                chain_id: num_bigint::BigInt::from(0),
                value: num_bigint::BigInt::from(0),
                is_init: false,
                datetime: chrono::DateTime::from_timestamp(0, 0).unwrap(),
            },
            entry_kind: public_abi::EntryKind::Main,
            entry_data: bytes::Bytes::new(),
            entry_stage_data: calldata::Value::Null,
        };
        let context = Context {
            data: SingleVMData {
                conf: conf.clone(),
                limiter: limiter.clone(),
                remaining_recursion: top_limits::VM_RECURSION,
                spawn_kind: "test".to_owned(),
                message_data,
                supervisor: supervisor.clone(),
                storage,
                accumulator: VMDataAccumulator {
                    data_fees_limit: shared_data.gep(|data| &data.data_fees_limit),
                    messages_value_decremented: U256::zero(),
                    emissions: Vec::new(),
                    message_fee_allocation: vec![
                        external_message_allocation(),
                        internal_message_allocation(),
                    ],
                    message_fee_allocation_consumed: vec![U256::zero(); 2],
                },
                det_subvm_hashes: Default::default(),
                granted_custom: Vec::new(),
            },
            loaded: Default::default(),
            limiter: limiter.clone(),
            start_time: std::time::Instant::now(),
            prev_time: std::time::Instant::now(),
        };

        Self {
            _host_peer: host_peer,
            vfs: vfs::VFS::new(Vec::new(), limiter).unwrap(),
            preview1: super::super::preview1::Context::new(
                chrono::DateTime::from_timestamp(0, 0).unwrap(),
                conf,
                [0; 32],
            ),
            context,
            _root: root,
        }
    }

    fn wasi(&mut self) -> ContextVFS<'_> {
        ContextVFS {
            vfs: &mut self.vfs,
            preview1: &mut self.preview1,
            context: &mut self.context,
        }
    }

    async fn emit_message(
        &mut self,
        emission: MessageEmission,
    ) -> Result<generated::types::Fd, generated::types::Error> {
        let mut wasi = self.wasi();
        match emission {
            MessageEmission::External => {
                wasi.gl_call_emit_external_message(
                    calldata::Address::zero(),
                    bytes::Bytes::new(),
                    U256::zero(),
                )
                .await
            }
            MessageEmission::InternalAllocation | MessageEmission::InternalBalance => {
                let use_balance = matches!(emission, MessageEmission::InternalBalance);
                wasi.gl_call_emit_internal_message(
                    calldata::Address::zero(),
                    abi::entry::MainCallData {
                        name: None,
                        args: None,
                        kwargs: None,
                    },
                    U256::zero(),
                    gl_call::On::Finalized,
                    use_balance,
                    use_balance.then(valid_params),
                )
                .await
            }
            MessageEmission::DeployAllocation | MessageEmission::DeployBalance => {
                let use_balance = matches!(emission, MessageEmission::DeployBalance);
                wasi.gl_call_emit_internal_deploy_message(
                    abi::entry::MainDeployData {
                        args: None,
                        kwargs: None,
                    },
                    gl_call::On::Finalized,
                    use_balance.then(valid_params),
                    super::message::EmitInternalDeployMessageArgs {
                        code: bytes::Bytes::new(),
                        value: U256::zero(),
                        salt_nonce: U256::zero(),
                        use_balance,
                    },
                )
                .await
            }
        }
    }

    async fn shutdown(self) {
        rt::supervisor::await_nondet_vms(&self.context.data.supervisor)
            .await
            .unwrap();
    }
}

fn trap_message(error: generated::types::Error) -> String {
    let trap: wasmtime::Error = error.downcast().expect_err("expected a trap");
    trap.to_string()
}

#[test]
fn emission_allocation_includes_fixed_and_payload_costs() {
    assert_eq!(
        emission_allocation_size(&[3, 5]),
        u64::from(memory_limiter_consts::EXECUTION_EMISSION_BASE_SIZE) + 8
    );
}

#[test]
fn emission_allocation_overflow_cannot_fit_the_budget() {
    assert_eq!(emission_allocation_size(&[u64::MAX]), u64::MAX);
}

#[test]
fn first_message_flag_ignores_events_and_flips_after_a_message() {
    let event = domain::ExecutionEmission::Event {
        topics: Vec::new(),
        blob: calldata::Map::new().into(),
        storage_fee: U256::zero(),
    };
    assert!(next_message_is_first(&[event]));

    let message = domain::ExecutionEmission::ExternalMessage {
        address: calldata::Address::zero(),
        calldata: bytes::Bytes::new(),
        value: U256::zero(),
        message_fee: U256::zero(),
        receipt_fee: U256::zero(),
        fee_params: abi::fees::ExternalMessageParams {
            gas_limit: U256::zero(),
            max_gas_price: U256::zero(),
        },
    };
    assert!(!next_message_is_first(&[message]));
}

#[tokio::test]
async fn messages_rejected_by_memory_are_not_appended_or_charged() {
    for emission in MessageEmission::ALL {
        let mut test = EmissionTestContext::new(0, 1);

        let error = test.emit_message(emission).await.unwrap_err();

        let message = trap_message(error);
        assert!(
            message.contains("out_of memory"),
            "unexpected error for {}: {message}",
            emission.name()
        );
        assert!(
            test.context.data.accumulator.emissions.is_empty(),
            "{} was appended",
            emission.name()
        );
        assert_eq!(
            test.context
                .data
                .supervisor
                .shared_data
                .data_fees_limit
                .remaining()
                .await,
            std::collections::BTreeMap::from([("test".to_owned(), U256::from(1))]),
            "{} was charged",
            emission.name()
        );
        assert!(
            test.context
                .data
                .accumulator
                .message_fee_allocation
                .iter()
                .all(|node| node.budget == U256::from(100)),
            "{} consumed its allocation budget",
            emission.name()
        );

        test.shutdown().await;
    }
}

#[tokio::test]
async fn messages_rejected_by_fee_are_not_appended_and_release_memory() {
    for emission in MessageEmission::ALL {
        let mut test = EmissionTestContext::new(u32::MAX, 0);
        let memory_before = test.context.limiter.get_remaining_memory();

        let error = test.emit_message(emission).await.unwrap_err();

        let message = trap_message(error);
        assert!(
            message.contains(emission.fee_error()),
            "unexpected error for {}: {message}",
            emission.name()
        );
        assert!(
            test.context.data.accumulator.emissions.is_empty(),
            "{} was appended",
            emission.name()
        );
        assert_eq!(
            test.context.limiter.get_remaining_memory(),
            memory_before,
            "{} retained memory",
            emission.name()
        );
        assert_eq!(
            test.context.limiter.get_new_permanent_allocations(),
            0,
            "{} committed memory",
            emission.name()
        );
        assert!(
            test.context
                .data
                .accumulator
                .message_fee_allocation
                .iter()
                .all(|node| node.budget == U256::from(100)),
            "{} consumed its allocation budget",
            emission.name()
        );
        let consumed = test
            .context
            .data
            .supervisor
            .shared_data
            .data_fees_limit
            .consumed()
            .await;
        assert_eq!(
            consumed.message_fee,
            U256::zero(),
            "{} consumed a message fee",
            emission.name()
        );
        assert_eq!(
            consumed.message_receipt,
            U256::zero(),
            "{} consumed a receipt fee",
            emission.name()
        );
        assert_eq!(
            test.context.data.accumulator.messages_value_decremented,
            U256::zero(),
            "{} consumed balance",
            emission.name()
        );

        test.shutdown().await;
    }
}

#[tokio::test]
async fn unallocated_external_receipt_exhaustion_is_classified_as_receipt() {
    let mut test = EmissionTestContext::new(u32::MAX, 0);
    test.context
        .data
        .accumulator
        .message_fee_allocation
        .retain(|node| {
            matches!(
                &node.fee_params,
                genvm_modules_interfaces::fees::MessageAllocationNodeParams::Internal(_)
            )
        });

    let error = test
        .emit_message(MessageEmission::External)
        .await
        .unwrap_err();

    assert!(trap_message(error).contains("out_of receipt message"));
    test.shutdown().await;
}

#[tokio::test]
async fn repeated_internal_messages_preserve_canonical_subtree_and_charge_budgets() {
    for emission in [
        MessageEmission::InternalAllocation,
        MessageEmission::DeployAllocation,
    ] {
        let mut test = EmissionTestContext::new(u32::MAX, 14);
        let grandchild = allocation_child(U256::one(), Vec::new());
        let first_child = allocation_child(U256::from(2), vec![grandchild]);
        let second_child = allocation_child(U256::from(3), Vec::new());
        test.context.data.accumulator.message_fee_allocation[1].children =
            vec![first_child, second_child];

        test.emit_message(emission).await.unwrap();
        test.emit_message(emission).await.unwrap();

        let emission_data = |emission: &domain::ExecutionEmission| match emission {
            domain::ExecutionEmission::InternalMessage {
                message_fee,
                subtree,
                ..
            }
            | domain::ExecutionEmission::InternalDeployMessage {
                message_fee,
                subtree,
                ..
            } => (*message_fee, subtree.clone()),
            other => panic!("unexpected emission: {other:?}"),
        };
        let first = emission_data(&test.context.data.accumulator.emissions[0]);
        let second = emission_data(&test.context.data.accumulator.emissions[1]);
        assert_eq!(first.0, U256::from(6), "{}", emission.name());
        assert_eq!(second.0, U256::from(6), "{}", emission.name());
        assert_eq!(first.1, second.1, "{} subtree changed", emission.name());
        assert_eq!(
            test.context.data.accumulator.message_fee_allocation[1].budget,
            U256::from(100),
            "{}",
            emission.name()
        );
        assert_eq!(
            test.context
                .data
                .accumulator
                .message_fee_allocation_consumed[1],
            U256::from(12),
            "{}",
            emission.name()
        );
        let consumed = test
            .context
            .data
            .supervisor
            .shared_data
            .data_fees_limit
            .consumed()
            .await;
        assert_eq!(consumed.message_fee, U256::from(12), "{}", emission.name());
        assert_eq!(
            consumed.message_receipt,
            U256::from(2),
            "{}",
            emission.name()
        );

        test.shutdown().await;
    }
}

#[tokio::test]
async fn child_budget_fee_failure_is_atomic() {
    let mut test = EmissionTestContext::new(u32::MAX, 6);
    test.context.data.accumulator.message_fee_allocation[1].children = vec![
        allocation_child(U256::from(2), Vec::new()),
        allocation_child(U256::from(3), Vec::new()),
    ];
    let memory_before = test.context.limiter.get_remaining_memory();

    let error = test
        .emit_message(MessageEmission::InternalAllocation)
        .await
        .unwrap_err();

    assert!(
        trap_message(error).contains("out_of message_fee total # internal"),
        "unexpected fee error"
    );
    assert!(test.context.data.accumulator.emissions.is_empty());
    assert_eq!(
        test.context.data.accumulator.message_fee_allocation[1].budget,
        U256::from(100)
    );
    assert_eq!(
        test.context
            .data
            .accumulator
            .message_fee_allocation_consumed[1],
        U256::zero()
    );
    assert_eq!(test.context.limiter.get_remaining_memory(), memory_before);
    let consumed = test
        .context
        .data
        .supervisor
        .shared_data
        .data_fees_limit
        .consumed()
        .await;
    assert_eq!(consumed.message_fee, U256::zero());
    assert_eq!(consumed.message_receipt, U256::zero());

    test.shutdown().await;
}

#[tokio::test]
async fn child_budget_overflow_is_internal_and_has_no_effect() {
    let mut test = EmissionTestContext::new(u32::MAX, 1);
    test.context.data.accumulator.message_fee_allocation[1].children =
        vec![allocation_child(U256::MAX, Vec::new())];
    let memory_before = test.context.limiter.get_remaining_memory();

    let error = test
        .emit_message(MessageEmission::InternalAllocation)
        .await
        .unwrap_err();

    assert!(
        trap_message(error).contains("message declared budget overflow"),
        "unexpected overflow error"
    );
    assert!(test.context.data.accumulator.emissions.is_empty());
    assert_eq!(
        test.context.data.accumulator.message_fee_allocation[1].budget,
        U256::from(100)
    );
    assert_eq!(
        test.context
            .data
            .accumulator
            .message_fee_allocation_consumed[1],
        U256::zero()
    );
    assert_eq!(test.context.limiter.get_remaining_memory(), memory_before);
    let consumed = test
        .context
        .data
        .supervisor
        .shared_data
        .data_fees_limit
        .consumed()
        .await;
    assert_eq!(consumed.message_fee, U256::zero());
    assert_eq!(consumed.message_receipt, U256::zero());

    test.shutdown().await;
}

#[tokio::test]
async fn event_rejected_by_memory_is_not_appended_or_charged() {
    let mut test = EmissionTestContext::new(0, 1);

    let error = test
        .wasi()
        .gl_call_emit_event(Vec::new(), calldata::Map::new().into())
        .await
        .unwrap_err();

    let message = trap_message(error);
    assert!(
        message.contains("out_of memory"),
        "unexpected error: {message}"
    );
    assert!(test.context.data.accumulator.emissions.is_empty());
    assert_eq!(
        test.context
            .data
            .supervisor
            .shared_data
            .data_fees_limit
            .remaining()
            .await,
        std::collections::BTreeMap::from([("test".to_owned(), U256::from(1))])
    );

    test.shutdown().await;
}

#[tokio::test]
async fn event_rejected_by_fee_is_not_appended_and_releases_memory() {
    let mut test = EmissionTestContext::new(u32::MAX, 0);
    let memory_before = test.context.limiter.get_remaining_memory();

    let error = test
        .wasi()
        .gl_call_emit_event(Vec::new(), calldata::Map::new().into())
        .await
        .unwrap_err();

    let message = trap_message(error);
    assert!(
        message.contains("out_of receipt event"),
        "unexpected error: {message}"
    );
    assert!(test.context.data.accumulator.emissions.is_empty());
    assert_eq!(test.context.limiter.get_remaining_memory(), memory_before);
    assert_eq!(test.context.limiter.get_new_permanent_allocations(), 0);
    assert_eq!(
        test.context
            .data
            .supervisor
            .shared_data
            .data_fees_limit
            .consumed()
            .await
            .event,
        U256::zero()
    );

    test.shutdown().await;
}

#[test]
fn nondet_ram_preflight_preserves_the_fallback_budget() {
    let memory_error = NondetOutput::vm_error(public_abi::VmError::out_of().memory().val());
    let fee_error = NondetOutput::vm_error(public_abi::VmError::out_of().receipt().nondet_output());
    let budget = memory_error
        .preflight_ram_size()
        .max(fee_error.preflight_ram_size()) as u32;
    let limiter = rt::memlimiter::Limiter::with_limit(budget);

    preflight_nondet_output_ram(&limiter, &memory_error, &fee_error).unwrap();

    assert_eq!(limiter.get_remaining_memory(), budget);
    assert_eq!(limiter.get_new_permanent_allocations(), 0);
}

#[test]
fn nondet_ram_preflight_rejects_a_budget_without_room_for_an_error() {
    let memory_error = NondetOutput::vm_error(public_abi::VmError::out_of().memory().val());
    let fee_error = NondetOutput::vm_error(public_abi::VmError::out_of().receipt().nondet_output());
    let required = memory_error
        .preflight_ram_size()
        .max(fee_error.preflight_ram_size()) as u32;
    let limiter = rt::memlimiter::Limiter::with_limit(required - 1);

    assert!(preflight_nondet_output_ram(&limiter, &memory_error, &fee_error).is_err());
    assert_eq!(limiter.get_remaining_memory(), required - 1);
}

#[test]
fn oversized_nondet_output_is_replaced_and_permanently_charged() {
    let memory_error = NondetOutput::vm_error(public_abi::VmError::out_of().memory().val());
    let oversized = NondetOutput::vm_error(public_abi::VmError(std::borrow::Cow::Owned(
        "x".repeat(256),
    )));
    let budget = memory_error.allocation_size() as u32;
    let limiter = rt::memlimiter::Limiter::with_limit(budget);

    let (output, allocation) = reserve_nondet_output(&limiter, oversized, &memory_error).unwrap();
    assert_eq!(output.encoded, memory_error.encoded);
    allocation.commit();

    assert_eq!(limiter.get_remaining_memory(), 0);
    assert_eq!(limiter.get_new_permanent_allocations(), budget);
}

#[test]
fn nondet_cap_replacement_preserves_a_fatal_leader_rejection() {
    let memory_error = NondetOutput::vm_error(public_abi::VmError::out_of().memory().val());
    let error = public_abi::VmError(std::borrow::Cow::Owned("x".repeat(256)));
    let encoded = rt::vm::ContractOutcome::VMError(error.clone(), None).encode();
    let rejected = NondetOutput {
        result: rt::vm::RunOk::FatalVMError(error, None),
        encoded,
    };
    let limiter = rt::memlimiter::Limiter::with_limit(memory_error.allocation_size() as u32);

    let (output, _) = reserve_nondet_output(&limiter, rejected, &memory_error).unwrap();

    assert!(matches!(output.result, rt::vm::RunOk::FatalVMError(..)));
    assert_eq!(output.encoded, memory_error.encoded);
}

#[tokio::test]
async fn nondet_fee_preflight_fails_before_consuming_the_fallback_fee() {
    let memory_error = NondetOutput::vm_error(public_abi::VmError::out_of().memory().val());
    let fee_error = NondetOutput::vm_error(public_abi::VmError::out_of().receipt().nondet_output());
    let required = fee_error.encoded.as_slice().len() as u64;
    let fees = nondet_fees(required - 1);

    assert!(
        preflight_nondet_output_fees(&fees, &memory_error, &fee_error)
            .await
            .is_err()
    );
    assert_eq!(
        fees.remaining().await,
        std::collections::BTreeMap::from([("test".to_owned(), U256::from(required - 1))])
    );
    assert_eq!(fees.consumed().await.nondet_output, U256::zero());
}

#[tokio::test]
async fn nondet_fee_preflight_checks_the_memory_error_with_non_monotone_fees() {
    let memory_error = NondetOutput::vm_error(public_abi::VmError::out_of().memory().val());
    let fee_error = NondetOutput::vm_error(public_abi::VmError::out_of().receipt().nondet_output());
    let fee_error_len = fee_error.encoded.as_slice().len();
    let fees = nondet_fees_with_delta(
        0,
        &format!("\\attrs = if attrs.outputLength < {fee_error_len} then 1 else 0"),
    );

    assert!(fees
        .can_consume_nondet_output(fee_error_len as u64)
        .await
        .unwrap());
    assert!(
        preflight_nondet_output_fees(&fees, &memory_error, &fee_error)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn over_cap_nondet_payload_is_replaced_before_publication() {
    let memory_error = NondetOutput::vm_error(public_abi::VmError::out_of().memory().val());
    let fee_error = NondetOutput::vm_error(public_abi::VmError::out_of().receipt().nondet_output());
    let oversized = NondetOutput::vm_error(public_abi::VmError(std::borrow::Cow::Owned(
        "x".repeat(256),
    )));
    let fee_error_len = fee_error.encoded.as_slice().len() as u64;
    let memory_budget = oversized.allocation_size() as u32;
    let limiter = rt::memlimiter::Limiter::with_limit(memory_budget);
    let fees = nondet_fees(fee_error_len);

    let output = charge_nondet_output(&limiter, &fees, oversized, &memory_error, &fee_error)
        .await
        .unwrap();

    assert_eq!(output.encoded, fee_error.encoded);
    assert_eq!(
        limiter.get_remaining_memory(),
        memory_budget - fee_error.allocation_size() as u32
    );
    assert_eq!(
        fees.remaining().await,
        std::collections::BTreeMap::from([("test".to_owned(), U256::zero())])
    );
    assert_eq!(
        fees.consumed().await.nondet_output,
        U256::from(fee_error_len)
    );
}

#[tokio::test]
async fn fee_capped_nondet_output_still_fits_a_readable_fd() {
    let memory_error = NondetOutput::vm_error(public_abi::VmError::out_of().memory().val());
    let fee_error = NondetOutput::vm_error(public_abi::VmError::out_of().receipt().nondet_output());
    let oversized = NondetOutput::vm_error(public_abi::VmError(std::borrow::Cow::Owned(
        "x".repeat(256),
    )));
    let budget = memory_error
        .preflight_ram_size()
        .max(fee_error.preflight_ram_size()) as u32;
    let limiter = rt::memlimiter::Limiter::with_limit(budget);
    let fees = nondet_fees(fee_error.encoded.as_slice().len() as u64);

    preflight_nondet_output_ram(&limiter, &memory_error, &fee_error).unwrap();
    let output = charge_nondet_output(&limiter, &fees, oversized, &memory_error, &fee_error)
        .await
        .unwrap();
    assert_eq!(output.encoded, fee_error.encoded);

    let mut vfs = vfs::VFS::new(Vec::new(), limiter.clone()).unwrap();
    vfs.place_content(vfs::FileContents::from(output.encoded.into_bytes()))
        .unwrap();
    assert_eq!(limiter.get_remaining_memory(), 0);
}

#[tokio::test]
async fn nondet_fee_cap_takes_precedence_when_both_caps_are_exceeded() {
    let memory_error = NondetOutput::vm_error(public_abi::VmError::out_of().memory().val());
    let fee_error = NondetOutput::vm_error(public_abi::VmError::out_of().receipt().nondet_output());
    let oversized = NondetOutput::vm_error(public_abi::VmError(std::borrow::Cow::Owned(
        "x".repeat(256),
    )));
    let limiter = rt::memlimiter::Limiter::with_limit(
        memory_error
            .preflight_ram_size()
            .max(fee_error.preflight_ram_size()) as u32,
    );
    let fees = nondet_fees(fee_error.encoded.as_slice().len() as u64);

    let output = charge_nondet_output(&limiter, &fees, oversized, &memory_error, &fee_error)
        .await
        .unwrap();

    assert_eq!(output.encoded, fee_error.encoded);
}

#[test]
fn validator_treats_a_post_cap_leader_mismatch_as_a_leader_fault() {
    let proposed = NondetOutput::vm_error(public_abi::VmError::timeout());
    let capped = NondetOutput::vm_error(public_abi::VmError::out_of().memory().val());

    assert!(validate_leader_output_after_caps(&proposed.encoded, &proposed.encoded).is_ok());
    assert_eq!(
        validate_leader_output_after_caps(&capped.encoded, &proposed.encoded).unwrap_err(),
        malformed()
    );
}

#[test]
fn balance_no_permission_is_forbidden() {
    let err = validate_balance_fee(false, true, Some(valid_params())).unwrap_err();
    assert_eq!(errno(err), generated::types::Errno::Forbidden);
}

#[test]
fn balance_without_params_is_inval() {
    let err = validate_balance_fee(true, true, None).unwrap_err();
    assert_eq!(errno(err), generated::types::Errno::Inval);
}

#[test]
fn params_without_use_balance_is_inval() {
    let err = validate_balance_fee(true, false, Some(valid_params())).unwrap_err();
    assert_eq!(errno(err), generated::types::Errno::Inval);
}

#[test]
fn empty_rotations_is_inval() {
    let mut p = valid_params();
    p.rotations.clear();
    let err = validate_balance_fee(true, true, Some(p)).unwrap_err();
    assert_eq!(errno(err), generated::types::Errno::Inval);
}

#[test]
fn zero_price_caps_are_inval() {
    for mutate in [
        (|p: &mut abi::fees::InternalMessageParams| p.max_price_gen_per_time_unit = U256::zero())
            as fn(&mut abi::fees::InternalMessageParams),
        |p| p.storage_fee_max_gas_price = U256::zero(),
        |p| p.receipt_fee_max_gas_price = U256::zero(),
    ] {
        let mut p = valid_params();
        mutate(&mut p);
        let err = validate_balance_fee(true, true, Some(p)).unwrap_err();
        assert_eq!(errno(err), generated::types::Errno::Inval);
    }
}

#[test]
fn huge_magnitude_params_are_inval() {
    // Security-review N1 repro: passes the emptiness/zero checks, but the
    // 2^250 magnitudes would push messageFeeFloor past U256 and saturate the
    // evaluator's result to U256::MAX.
    let p = abi::fees::InternalMessageParams {
        leader_time_units_allocation: U256::one() << 250,
        validator_time_units_allocation: U256::zero(),
        execution_budget_per_round: U256::zero(),
        rotations: vec![U256::zero()],
        max_price_gen_per_time_unit: U256::one() << 250,
        storage_fee_max_gas_price: U256::from(20),
        receipt_fee_max_gas_price: U256::from(20),
    };
    let err = validate_balance_fee(true, true, Some(p)).unwrap_err();
    assert_eq!(errno(err), generated::types::Errno::Inval);
}

#[test]
fn huge_rotations_entry_is_inval() {
    let mut p = valid_params();
    p.rotations[2] = U256::one() << 250;
    let err = validate_balance_fee(true, true, Some(p)).unwrap_err();
    assert_eq!(errno(err), generated::types::Errno::Inval);
}

#[test]
fn params_at_magnitude_bounds_pass() {
    let mut p = valid_params();
    p.max_price_gen_per_time_unit = (U256::one() << FEE_PARAM_PRICE_BITS) - 1;
    p.storage_fee_max_gas_price = (U256::one() << FEE_PARAM_PRICE_BITS) - 1;
    p.receipt_fee_max_gas_price = (U256::one() << FEE_PARAM_PRICE_BITS) - 1;
    p.execution_budget_per_round = (U256::one() << FEE_PARAM_PRICE_BITS) - 1;
    p.leader_time_units_allocation = (U256::one() << FEE_PARAM_COUNT_BITS) - 1;
    p.validator_time_units_allocation = (U256::one() << FEE_PARAM_COUNT_BITS) - 1;
    p.rotations = vec![(U256::one() << FEE_PARAM_COUNT_BITS) - 1; 5];
    let got = validate_balance_fee(true, true, Some(p.clone())).unwrap();
    assert_eq!(got, Some(p));
}

#[test]
fn valid_balance_params_pass_through() {
    let p = valid_params();
    let got = validate_balance_fee(true, true, Some(p.clone())).unwrap();
    assert_eq!(got, Some(p));
}

#[test]
fn no_balance_no_params_is_allocation_path() {
    let got = validate_balance_fee(true, false, None).unwrap();
    assert_eq!(got, None);
}

#[test]
fn non_null_resolve_selects_nested_path_before_local_spawn() {
    let payload = bytes::Bytes::from_static(b"route");
    let ours = genvm_common::version::CURRENT.major as u8;
    assert_eq!(
        call_contract_route(Some(payload.clone()), ours),
        CallContractRoute::Nested(payload)
    );
    assert_eq!(
        call_contract_route(None, ours),
        CallContractRoute::InProcess
    );
}

#[test]
fn a_major_this_line_does_not_serve_goes_to_the_manager() {
    let unservable = genvm_common::version::CURRENT.major as u8 + 1;
    let CallContractRoute::Nested(payload) = call_contract_route(None, unservable) else {
        panic!("a major this line cannot serve must not stay in-process");
    };
    let routing: genvm_modules_interfaces::ExecutorSelector =
        calldata::decode_obj(&payload).unwrap();
    assert_eq!(
        routing,
        genvm_modules_interfaces::ExecutorSelector::MajorOverride {
            major: unservable as u32
        }
    );
}

#[test]
fn call_contract_child_is_deterministic_only() {
    let parent = base::Permissions {
        deterministic: true,
        write_storage: true,
        send_messages: true,
        call_others: true,
        spawn_nondet: true,
        can_use_balance_for_message_fees: true,
    };
    let child = derive_call_contract_permissions(&parent);

    assert!(child.deterministic);
    assert!(child.call_others);
    assert!(!child.spawn_nondet);
    assert!(!child.write_storage);
    assert!(!child.send_messages);
    assert!(!child.can_use_balance_for_message_fees);
}

#[test]
fn nested_result_must_be_effect_free() {
    let reply = genvm_modules_interfaces::NestedRunReply {
        result: genvm_modules_interfaces::NestedRunResult {
            kind: genvm_modules_interfaces::ResultCode::Return,
            data: calldata::Value::Null.into(),
        },
        small_hash: bytes::Bytes::from(vec![1; 32]),
        effect_free: false,
    };

    assert!(nested_run_ok(reply).is_err());
}

// ---------------------------------------------------------------------------
// Leader-proposed nondet result validation.
//
// Every case below is leader-authored input on the validator path. None of them
// may trap: a trap would turn "the leader sent garbage" into a validator
// internal error (and a timeout vote), which is a leader-controlled way to
// silence validators. They must all resolve to a VMError the caller can turn
// into a disagreement.
// ---------------------------------------------------------------------------

fn leader_bytes(code: public_abi::ResultCode, payload: &[u8]) -> Vec<u8> {
    let mut res = vec![code as u8];
    res.extend_from_slice(payload);
    res
}

fn malformed() -> public_abi::VmError {
    public_abi::VmError::leader_fault()
        .nondet_output()
        .malformed()
}

#[test]
fn leader_empty_result_is_absent() {
    assert_eq!(
        parse_leader_result(&[]).unwrap_err(),
        public_abi::VmError::leader_fault().nondet_output().absent()
    );
}

#[test]
fn leader_unknown_result_code_is_malformed() {
    for code in [7u8, 0x80, 0xff] {
        assert_eq!(parse_leader_result(&[code]).unwrap_err(), malformed());
    }
}

#[test]
fn leader_internal_error_code_is_malformed() {
    // A proposable result admits only 0/1/2; the host-facing codes share the
    // numbering but are never proposable.
    for code in [
        crate::host::host_fns::ResultCode::InternalError,
        crate::host::host_fns::ResultCode::FatalVmError,
    ] {
        assert_eq!(parse_leader_result(&[code as u8]).unwrap_err(), malformed());
    }
}

#[test]
fn fatal_leader_outcome_cannot_be_published() {
    let result = rt::vm::RunOk::FatalVMError(public_abi::VmError::timeout(), None);

    assert!(leader_outcome_for_publication(result).is_err());
}

#[test]
fn malformed_leader_outcome_is_rejected_and_charged_as_a_vm_error() {
    let proposal =
        leader_proposal_for_validation(&[crate::host::host_fns::ResultCode::FatalVmError as u8]);

    assert!(matches!(proposal, LeaderProposal::Rejected(..)));

    let (returned, encoded) = proposal.into_result_and_encoding();

    assert!(matches!(returned, rt::vm::RunOk::FatalVMError(..)));
    assert_eq!(encoded.as_slice()[0], public_abi::ResultCode::VmError as u8);
}

#[test]
fn leader_return_with_invalid_calldata_is_malformed() {
    // The hole this closes: the executor used to pass the `Return` payload
    // through undecoded, so bytes like these reached the execution hash before
    // the guest SDK ever got a chance to reject them.
    let data = leader_bytes(public_abi::ResultCode::Return, &[0xff, 0xff, 0xff]);
    assert_eq!(parse_leader_result(&data).unwrap_err(), malformed());
}

#[test]
fn leader_return_with_trailing_bytes_is_malformed() {
    let mut payload = calldata::encode(&calldata::Value::Null);
    payload.push(0x00);
    let data = leader_bytes(public_abi::ResultCode::Return, &payload);
    assert_eq!(parse_leader_result(&data).unwrap_err(), malformed());
}

#[test]
fn leader_return_exceeding_depth_limit_is_malformed() {
    // The undecoded `Return` path bypasses the decoder's depth limit entirely.
    let mut v = calldata::Value::Null;
    for _ in 0..200 {
        v = calldata::Value::Array(vec![v]);
    }
    let data = leader_bytes(public_abi::ResultCode::Return, &calldata::encode(&v));
    assert_eq!(parse_leader_result(&data).unwrap_err(), malformed());
}

#[test]
fn leader_valid_return_is_preserved_bytewise() {
    let value = calldata::Value::Str("ok".to_owned());
    let payload = calldata::encode(&value);
    let data = leader_bytes(public_abi::ResultCode::Return, &payload);

    let got = parse_leader_result(&data).unwrap();
    // Validation must not re-encode: the accepted result has to round-trip to the
    // exact bytes the leader proposed, or validators would hash a different
    // value than the one they agreed to.
    assert_eq!(got.encode().into_bytes().as_ref(), data);
}

#[test]
fn leader_user_error_with_invalid_calldata_is_malformed() {
    let data = leader_bytes(public_abi::ResultCode::UserError, &[0xff, 0xff]);
    assert_eq!(parse_leader_result(&data).unwrap_err(), malformed());
}

#[test]
fn leader_valid_user_error_passes() {
    let payload = calldata::encode(&calldata::Value::Str("boom".to_owned()));
    let data = leader_bytes(public_abi::ResultCode::UserError, &payload);
    assert_eq!(
        parse_leader_result(&data)
            .unwrap()
            .encode()
            .into_bytes()
            .as_ref(),
        data
    );
}

#[test]
fn leader_vm_error_not_utf8_is_malformed() {
    let data = leader_bytes(public_abi::ResultCode::VmError, &[0xff, 0xfe]);
    assert_eq!(parse_leader_result(&data).unwrap_err(), malformed());
}

#[test]
fn leader_vm_error_off_trie_code_is_malformed() {
    // A leader may not invent error codes: the public code must come from the
    // `vm_error` trie.
    for code in [
        "i_made_this_up",
        "timeou",
        "timeoutx",
        "out_of",
        "out_of nonsense",
        "exit_code",
        "exit_code notanumber",
        "exit_code 99999999999999999999",
        "wasm_trap not_a_trap",
        "",
    ] {
        let data = leader_bytes(public_abi::ResultCode::VmError, code.as_bytes());
        assert_eq!(
            parse_leader_result(&data).unwrap_err(),
            malformed(),
            "code {code:?} should be rejected"
        );
    }
}

#[test]
fn leader_vm_error_on_trie_code_passes() {
    for code in [
        "timeout",
        "forbidden",
        "out_of storage",
        "out_of memory",
        "out_of memory wasm_memory",
        "invalid_contract",
        "invalid_contract wasm linking",
        "exit_code 1",
        "exit_code -1",
        "wasm_trap",
        "wasm_trap unreachable",
    ] {
        let data = leader_bytes(public_abi::ResultCode::VmError, code.as_bytes());
        let got = parse_leader_result(&data)
            .unwrap_or_else(|e| panic!("code {code:?} should be accepted, got {e:?}"));
        assert_eq!(got.encode().into_bytes().as_ref(), data);
    }
}

#[test]
fn leader_vm_error_with_detail_is_malformed() {
    // The nondet result channel is detail-free: an honest leader strips its own
    // detail before publishing, so a proposal carrying one is malformed -- not
    // something to strip and accept. Accepting it would reopen a free-form byte
    // channel into the execution hash.
    for code in [
        "timeout # took too long",
        "made_up # detail",
        "timeout # ",
        " # ",
    ] {
        let data = leader_bytes(public_abi::ResultCode::VmError, code.as_bytes());
        assert_eq!(
            parse_leader_result(&data).unwrap_err(),
            malformed(),
            "code {code:?} should be rejected for carrying a detail"
        );
    }
}

#[test]
fn leader_vm_error_exit_code_must_be_canonical() {
    // `+7` and `007` parse as 7, so accepting them would let several
    // byte-different codes name the same error and hash differently.
    for code in [
        "exit_code +7",
        "exit_code 007",
        "exit_code -0",
        "exit_code 2147483648",
    ] {
        let data = leader_bytes(public_abi::ResultCode::VmError, code.as_bytes());
        assert_eq!(
            parse_leader_result(&data).unwrap_err(),
            malformed(),
            "code {code:?} should be rejected as non-canonical"
        );
    }

    for code in [
        "exit_code 0",
        "exit_code 7",
        "exit_code -7",
        "exit_code 2147483647",
    ] {
        let data = leader_bytes(public_abi::ResultCode::VmError, code.as_bytes());
        assert!(
            parse_leader_result(&data).is_ok(),
            "code {code:?} should be accepted"
        );
    }
}

#[test]
fn leader_vm_error_detail_is_rejected_before_the_namespace_remap() {
    // The one branch interaction the two checks have: a derived-namespace code
    // carrying a detail is `malformed`, not a remap. Order matters because the
    // remap hashes the whole proposed string, so running it first would fold an
    // attacker-chosen detail into the derived code.
    for code in [
        "leader_fault nondet_output malformed # x",
        "leader_fault nondet_output absent # x",
    ] {
        let data = leader_bytes(public_abi::ResultCode::VmError, code.as_bytes());
        assert_eq!(
            parse_leader_result(&data).unwrap_err(),
            malformed(),
            "code {code:?}: the detail check must win"
        );
    }
}

#[test]
fn leader_empty_payloads_are_malformed() {
    // A bare result code with no payload: `Return`/`UserError` need at least the
    // encoding of some value, so the empty tail fails the decoder.
    for code in [
        public_abi::ResultCode::Return,
        public_abi::ResultCode::UserError,
    ] {
        assert_eq!(
            parse_leader_result(&leader_bytes(code, &[])).unwrap_err(),
            malformed(),
            "{code:?} with an empty payload should be rejected"
        );
    }
}

#[test]
fn leader_user_error_malformed_payloads_match_return() {
    // `UserError` shares `Return`'s branch shape and must reject the same
    // payloads -- trailing bytes and over-deep nesting included.
    let mut trailing = calldata::encode(&calldata::Value::Null);
    trailing.push(0x00);

    let mut deep = calldata::Value::Null;
    for _ in 0..200 {
        deep = calldata::Value::Array(vec![deep]);
    }

    for payload in [trailing, calldata::encode(&deep)] {
        let data = leader_bytes(public_abi::ResultCode::UserError, &payload);
        assert_eq!(parse_leader_result(&data).unwrap_err(), malformed());
    }
}

#[test]
fn leader_vm_error_in_derived_namespace_is_remapped() {
    // A proposal must never be byte-equal to what a validator derives from
    // rejecting it, or "the leader proposed X" and "the proposal was rejected
    // as X" would be indistinguishable.
    for code in [
        "leader_fault nondet_output absent",
        "leader_fault nondet_output",
        "leader_fault nondet_output malformed",
        "leader_fault nondet_output uses_this_error abcdef",
        "leader_fault nondet_output whatever it wants",
        "leader_fault nondet_output absent extra",
    ] {
        let data = leader_bytes(public_abi::ResultCode::VmError, code.as_bytes());
        let err = parse_leader_result(&data).unwrap_err();

        assert_eq!(
            err,
            rt::errors::vm_error_for_leader_use_this_error(code),
            "code {code:?} should be remapped out of the derived namespace"
        );
        assert_ne!(
            err.0, code,
            "the derived outcome must not be byte-equal to the proposal"
        );
    }
}

#[test]
fn derived_outcomes_are_not_proposable_verbatim() {
    // Feeding a validator-derived outcome back in as a proposal must be
    // rejected again -- the namespace is closed under this loop, which is what
    // the `fix_point` sentinel exists for.
    for seed in ["", "timeout", "leader_fault nondet_output malformed"] {
        let derived = rt::errors::vm_error_for_leader_use_this_error(seed);
        let data = leader_bytes(public_abi::ResultCode::VmError, derived.0.as_bytes());
        assert!(
            parse_leader_result(&data).is_err(),
            "derived outcome {:?} must not be proposable",
            derived.0
        );
    }
}

#[test]
fn leader_vm_error_derived_entry_code_is_malformed() {
    // `malformed_entry` is an executor-derived outcome, so a leader may not
    // claim it as its own result.
    let code = public_abi::VmError::malformed_entry();
    let data = leader_bytes(public_abi::ResultCode::VmError, code.0.as_bytes());
    assert_eq!(parse_leader_result(&data).unwrap_err(), malformed());
}

#[test]
fn every_generated_trie_code_is_accepted() {
    // Guards against codegen drift: adding a `vm_error` trie entry without
    // regenerating `is_valid` would silently make a legitimate leader code
    // unproposable. Mirrors the constructors the generator emits.
    //
    // The leader-fault nondet-output subtree is deliberately absent: it lives
    // in the derived-outcome namespace and so is never proposable (see
    // `leader_vm_error_in_derived_namespace_is_remapped`).
    let codes = [
        public_abi::VmError::timeout(),
        public_abi::VmError::forbidden(),
        public_abi::VmError::wasm_trap().val(),
        public_abi::VmError::wasm_trap().unreachable(),
        public_abi::VmError::out_of().storage(),
        public_abi::VmError::out_of().memory().val(),
        public_abi::VmError::out_of().memory().wasm_memory(),
        public_abi::VmError::out_of().receipt().nondet_output(),
        public_abi::VmError::out_of().message_fee().total().val(),
        public_abi::VmError::fee().below_minimum(),
        public_abi::VmError::evm().reverted(),
        public_abi::VmError::invalid_contract().val(),
        public_abi::VmError::invalid_contract().wasm().linking(),
        public_abi::VmError::exit_code().val_i32(3),
    ];

    for code in codes {
        assert!(
            public_abi::VmError::is_valid_(&code.0),
            "generated code {:?} must pass its own trie check",
            code.0
        );

        let data = leader_bytes(public_abi::ResultCode::VmError, code.0.as_bytes());
        assert!(
            parse_leader_result(&data).is_ok(),
            "generated code {:?} should be proposable",
            code.0
        );
    }
}

// ---------------------------------------------------------------------------
// Publishing the leader's own result.
// ---------------------------------------------------------------------------

#[test]
fn leader_own_error_loses_only_its_detail() {
    assert_eq!(
        strip_vm_error_detail("timeout # took too long").0,
        "timeout"
    );
    assert_eq!(strip_vm_error_detail("timeout").0, "timeout");
    // Only the first separator splits: a detail may itself contain " # ".
    assert_eq!(
        strip_vm_error_detail("exit_code 1 # a # b").0,
        "exit_code 1"
    );
}

#[test]
fn leader_own_error_is_not_self_filtered() {
    // The acceptance check must not run on the leader's own output: rewriting an
    // honest result into a derived-namespace code would guarantee a hash mismatch
    // against honest validators, which unconditionally replace it again.
    let stripped = strip_vm_error_detail("leader_fault nondet_output absent # inner");
    assert_eq!(stripped.0, "leader_fault nondet_output absent");
    assert!(
        parse_leader_result(&leader_bytes(
            public_abi::ResultCode::VmError,
            stripped.0.as_bytes()
        ))
        .is_err(),
        "sanity: acceptance would have rewritten this code, the publish path must not"
    );
}

#[test]
fn every_stripped_leader_error_round_trips_through_acceptance() {
    // The invariant the publish path relies on: an honest leader's published
    // bytes are exactly what an honest validator accepts. Only the derived
    // namespace is exempt, and the leader cannot reach it (the nondet child
    // cannot spawn a nondet child of its own).
    for code in [
        "timeout",
        "forbidden",
        "wasm_trap unreachable",
        "out_of memory wasm_memory",
        "exit_code 0",
        "exit_code -7",
        "invalid_contract wasm linking",
    ] {
        let published = strip_vm_error_detail(&format!("{code} # some detail"));
        let data = leader_bytes(public_abi::ResultCode::VmError, published.0.as_bytes());

        let accepted = parse_leader_result(&data)
            .unwrap_or_else(|e| panic!("honest leader code {code:?} rejected as {e:?}"));
        assert_eq!(accepted.encode().into_bytes().as_ref(), data);
    }
}
