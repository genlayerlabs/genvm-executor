use std::{
    io::{Read, Write},
    os::fd::FromRawFd,
};

use genvm_common::*;

use anyhow::{Context, Result};
use genvm::{
    config,
    rt::{self},
};

const EXECUTION_DATA_HELP: &str = "path to file containing encoded execution data (use '-' for stdin, 'fd://N' for file descriptor N)";

fn fill_nested_fee_buckets(
    is_nested: bool,
    bucket_names: &[symbol_table::GlobalSymbol],
    bucket_totals: &mut std::collections::HashMap<String, primitive_types::U256>,
) {
    if is_nested && bucket_totals.is_empty() {
        bucket_totals.extend(
            bucket_names
                .iter()
                .copied()
                .map(|name| (name.as_str().to_owned(), primitive_types::U256::zero())),
        );
    }
}

/// Rejects permissions a nested run cannot act on whatever its caller derived.
///
/// A delegated run has no module connection, no fee allocation and no writable
/// storage, so accepting one of these would let it start and then fail at the
/// first use. The manager checks the same thing when it builds the envelope;
/// this executor does not get to assume that whoever spawned it did.
fn check_nested_permissions(
    permissions: genvm_modules_interfaces::NestedPermissions,
) -> Result<()> {
    use genvm_modules_interfaces::NestedPermissions as P;

    for (bit, name) in [
        (P::SPAWN_NONDET, "spawn_nondet"),
        (P::WRITE_STORAGE, "write_storage"),
        (P::SEND_MESSAGES, "send_messages"),
        (
            P::USE_BALANCE_FOR_MESSAGE_FEES,
            "use_balance_for_message_fees",
        ),
    ] {
        anyhow::ensure!(
            !permissions.contains(bit),
            "nested execution data asserts `{name}`, which a run crossing a major boundary never carries"
        );
    }

    Ok(())
}

#[derive(clap::Args, Debug)]
pub struct Args {
    #[arg(
        long,
        value_enum,
        default_value = "disabled",
        help = "debug level: disabled|safe|safe-unbounded|unsafe|unsafe-tracing. safe captures (bounded); safe-unbounded adds unbounded capture and tracing; unsafe adds `:latest`/`:test` resolution (diverges across machines); unsafe-tracing adds real time metering (non-deterministic)"
    )]
    debug_mode: genvm_common::DebugMode,

    #[arg(long, default_value = "-", help = EXECUTION_DATA_HELP)]
    execution_data: String,
    #[arg(
        long,
        help = "host connections in format id=uri (e.g. 0=unix:///path or 1=fd://N)"
    )]
    host: Vec<String>,
    #[arg(long, help = "id to pass to modules, useful for aggregating logs")]
    genvm_id: Option<u64>,
    #[clap(long, default_value_t = false)]
    sync: bool,
    #[clap(
        long,
        default_value = "rwscn",
        help = "r?w?s?c?n?u?, read/write/send messages/call contracts/spawn nondet/register runners"
    )]
    permissions: String,
    #[clap(long, help = "override LLM module address from config")]
    module_llm: Option<String>,
    #[clap(long, help = "override web module address from config")]
    module_web: Option<String>,
}

pub fn handle(args: Args, mut config: config::Config) -> Result<()> {
    // Apply CLI overrides for module addresses
    if let Some(llm_addr) = &args.module_llm {
        config.modules.llm.address = llm_addr.clone();
    }
    if let Some(web_addr) = &args.module_web {
        config.modules.web.address = web_addr.clone();
    }

    // Read execution data from file path, stdin, or file descriptor
    let execution_data_bytes = if args.execution_data == "-" {
        let mut buffer = Vec::new();
        std::io::stdin()
            .read_to_end(&mut buffer)
            .with_context(|| "reading execution data from stdin")?;
        buffer
    } else if let Some(fd_str) = args.execution_data.strip_prefix("fd://") {
        let fd: i32 = fd_str
            .parse()
            .with_context(|| format!("parsing file descriptor number from '{fd_str}'"))?;
        let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)
            .with_context(|| format!("reading execution data from fd {fd}"))?;
        std::mem::drop(file);
        buffer
    } else {
        std::fs::read(&args.execution_data)
            .with_context(|| format!("reading execution data from {}", args.execution_data))?
    };

    let execution_data =
        calldata::decode_obj::<genvm_modules_interfaces::ExecutionData>(&execution_data_bytes)
            .with_context(|| "decoding execution data")?;
    let message = &execution_data.message;
    let host_data = rt::parse_host_data(&execution_data)?;

    let can_write_storage = match &execution_data.nested {
        Some(nested) => {
            check_nested_permissions(nested.permissions)?;
            nested
                .permissions
                .contains(genvm_modules_interfaces::NestedPermissions::WRITE_STORAGE)
        }
        None => args.permissions.contains('w'),
    };
    let is_nested = execution_data.nested.is_some();

    let runtime = config
        .base
        .create_rt()
        .with_context(|| "creating tokio runtime")?;

    let genvm_id = match &args.genvm_id {
        None => {
            let mut random_bytes = [0; 8];
            let _ = getrandom::fill(&mut random_bytes);
            u64::from_le_bytes(random_bytes)
        }
        Some(v) => *v,
    };

    if args.host.is_empty() {
        anyhow::bail!("at least one --host must be specified");
    }

    let mut metrics = genvm::Metrics::default();
    let metrics_for_each_host = args
        .host
        .iter()
        .map(|_| genvm::host::Metrics::default())
        .collect::<Vec<_>>();
    metrics.hosts = Box::from(metrics_for_each_host);

    let mut bucket_totals = execution_data
        .bucket_totals
        .iter()
        .map(|(name, bi)| {
            anyhow::ensure!(!name.is_empty(), "bucket name must not be empty");
            let (sign, bytes) = bi.to_bytes_be();
            anyhow::ensure!(
                sign != num_bigint::Sign::Minus,
                "bucket total must be non-negative"
            );
            anyhow::ensure!(bytes.len() <= 32, "bucket total exceeds U256 range");
            let mut buf = [0u8; 32];
            let start = 32usize.saturating_sub(bytes.len());
            buf[start..].copy_from_slice(&bytes);
            Ok((name.clone(), primitive_types::U256::from_big_endian(&buf)))
        })
        .collect::<Result<std::collections::HashMap<_, _>>>()?;

    let bucket_names = [
        &config.fees.storage.buckets,
        &config.fees.message_receipt.buckets,
        &config.fees.nondet_output.buckets,
        &config.fees.message_fee.buckets,
        &config.fees.event.buckets,
    ]
    .into_iter()
    .flat_map(|v| v.iter().copied())
    .collect::<Vec<_>>();

    fill_nested_fee_buckets(is_nested, &bucket_names, &mut bucket_totals);
    for name in bucket_names {
        anyhow::ensure!(
            bucket_totals.contains_key(name.as_str()),
            "fees config references missing bucket `{}`",
            name.as_str(),
        );
    }

    let emit_leader_public_data =
        !args.sync && !is_nested && execution_data.leader_public_data.is_none();
    let (leader_nondet_results, malformed_leader_public_data) =
        match execution_data.leader_public_data.as_ref() {
            None => (None, false),
            Some(encoded) => match genvm::leader_public_data::LeaderPublicData::decode(encoded) {
                Ok(data) => (Some(data.nd_outs), false),
                Err(_) => (Some(Vec::new()), true),
            },
        };

    let shared_data = sync::DArc::new(genvm::rt::SharedData {
        is_sync: args.sync,
        genvm_id: genvm_modules_interfaces::GenVMId(genvm_id),
        debug_mode: args.debug_mode,
        metrics,
        data_fees_limit: genvm::rt::fees::DataLimit::new(
            bucket_totals,
            config.fees.clone(),
            execution_data.gas_data.clone(),
        )?,
        det_fuel_budget: genvm::rt::DetFuelBudget::new(
            execution_data.nested.as_ref().map(|n| n.remaining_det_fuel),
        ),
        llm_consumption: tokio::sync::Mutex::new(primitive_types::U256::zero()),
    });

    let mut hosts: Vec<genvm::Host> = args
        .host
        .iter()
        .enumerate()
        .map(|(id, uri)| {
            let metrics = shared_data.gep(|x| &x.metrics.hosts[id]);
            let hello_data = execution_data
                .host_hello_data
                .get(id)
                .map_or(&[][..], bytes::Bytes::as_ref);
            genvm::Host::connect(uri, metrics, hello_data)
                .with_context(|| format!("connecting host {id} to {uri}"))
        })
        .collect::<Result<_>>()?;

    let method_hosts = execution_data.method_hosts.clone();

    // Validate every method->host mapping up front, once, so all later host
    // selection (here and inside the VM) can index `hosts` safely instead of
    // panicking on a misconfigured index.
    if let Some(&bad) = method_hosts.iter().find(|&&h| h as usize >= hosts.len()) {
        anyhow::bail!(
            "method_hosts references host index {bad} but only {} host(s) are configured",
            hosts.len()
        );
    }

    let setup_run_ok = if malformed_leader_public_data {
        Some(genvm::rt::vm::RunOk::VMError(
            genvm::public_abi::VmError(std::borrow::Cow::Borrowed(
                "leader_fault nondet_output malformed",
            )),
            None,
        ))
    } else {
        // Nested execution was already charged by its parent. For top-level
        // execution, surface an insufficient startup bucket as a normal receipt.
        let insufficient_err = if is_nested {
            None
        } else {
            runtime.block_on(shared_data.data_fees_limit.consume_initial())
        };
        insufficient_err.map(|err| {
            err.into_run_ok().unwrap_or_else(|e| {
                genvm::rt::vm::RunOk::VMError(
                    genvm::public_abi::VmError::oom().val(),
                    Some(anyhow::Error::new(e)),
                )
            })
        })
    };
    if let Some(setup_run_ok) = setup_run_ok {
        let host_for = |method: genvm::host::host_fns::Methods| -> usize {
            let m = method as usize;
            if m < method_hosts.len() {
                method_hosts[m] as usize
            } else {
                0
            }
        };

        let data_fees_remaining = runtime.block_on(shared_data.data_fees_limit.remaining());
        let data_fees_consumed = runtime.block_on(shared_data.data_fees_limit.consumed());
        let leader_public_data = if emit_leader_public_data {
            genvm::leader_public_data::LeaderPublicData {
                nd_outs: Vec::new(),
            }
            .encode()
        } else {
            bytes::Bytes::new()
        };
        let result: Result<genvm::host::FullResult> = Ok(genvm::host::FullResult::new(
            genvm::rt::vm::FullResult::empty_from(setup_run_ok),
            leader_public_data,
            None,
            data_fees_remaining,
            data_fees_consumed,
            primitive_types::U256::zero(),
        ));

        hosts[host_for(genvm::host::host_fns::Methods::ConsumeResult)]
            .consume_result(&result)
            .context("consume result")?;
        for host in &mut hosts {
            host.flush().context("flush host before exit")?;
        }

        runtime.shutdown_timeout(std::time::Duration::from_millis(30));

        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();

        return Ok(());
    }

    let mut perm_size = 0;
    for perm in ["w", "s", "c", "n", "u"] {
        if args.permissions.contains(perm) {
            perm_size += 1;
        }
    }

    if perm_size != args.permissions.len() {
        anyhow::bail!("Invalid permissions {}", &args.permissions)
    }

    log_info!(genvm_id = genvm_id; "genvm id");

    let rt = runtime.enter();

    let supervisor = genvm::create_supervisor(
        &config,
        hosts,
        genvm::CreateSupervisorNamedArgs {
            method_hosts,
            gas_data: execution_data.gas_data.clone(),
            initial_time_units_allocation: execution_data.initial_time_units_allocation,
            leader_nondet_results,
            emit_leader_public_data,
            memory_limit: execution_data.nested.as_ref().map(|n| n.memory_limit),
            can_write_storage,
        },
        host_data,
        shared_data,
        message,
    )
    .with_context(|| format!("creating supervisor for genvm_id {genvm_id}"))?;

    std::mem::drop(rt);

    let res = runtime
        .block_on(genvm::run_with(
            execution_data,
            supervisor.clone(),
            &args.permissions,
        ))
        .with_context(|| "running genvm");

    if let Err(err) = &res {
        log_error!(error:ah = err; "error running genvm");
    }

    runtime.block_on(async {
        supervisor.modules.llm.close().await;
        supervisor.modules.web.close().await;
    });

    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();

    runtime.shutdown_timeout(std::time::Duration::from_millis(30));

    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();

    Ok(())
}
