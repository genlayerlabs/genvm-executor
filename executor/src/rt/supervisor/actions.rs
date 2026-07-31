use std::collections::{BTreeMap, HashSet, VecDeque};

use crate::{caching, public_abi, rt, runners};

use anyhow::Context as _;
use genlayer_sdk::abi;
use genvm_common::*;
use rt::errors::ResultExt as _;
use wiggle::error::Context as _;

pub struct Ctx<'a, 'b> {
    pub env: BTreeMap<String, String>,
    pub visited: HashSet<symbol_table::GlobalSymbol>,
    pub topmost_runner_id: runners::Id,
    pub supervisor: &'a rt::supervisor::Supervisor,
    pub vm: &'b mut rt::vm::VMBase,
}

fn make_malformed_runner_error(extra_msg: &str) -> anyhow::Error {
    rt::errors::Error::wrap(
        public_abi::VmError::invalid_contract().malformed_runner(),
        anyhow::anyhow!("{}", extra_msg),
    )
    .into()
}

fn next_action_context(
    capture: genvm_common::debug_mode::Capture,
    contexts: &VecDeque<String>,
    context: String,
) -> VecDeque<String> {
    match capture {
        genvm_common::debug_mode::Capture::Disabled => VecDeque::new(),
        genvm_common::debug_mode::Capture::Unbounded => {
            let mut contexts = contexts.clone();
            contexts.push_back(context);
            contexts
        }
        genvm_common::debug_mode::Capture::Bounded => {
            const CONTEXT_LIMIT: usize = 16;

            let mut contexts = contexts.clone();
            contexts.push_back(context);
            if contexts.len() >= CONTEXT_LIMIT {
                if contexts.get(1).map(String::as_str) == Some("...") {
                    contexts.remove(2);
                } else {
                    contexts[1] = "...".to_owned();
                    while contexts.len() > CONTEXT_LIMIT {
                        contexts.remove(2);
                    }
                }
            }
            contexts
        }
    }
}

fn maps_into_vm(to: &str) -> bool {
    to.split('/').find(|component| !component.is_empty()) == Some("vm")
}

/// Rejects mapping targets that are forbidden or could escape their intended
/// location. `maps_into_vm` only inspects the first component, so a `..`
/// component (e.g. from an archive entry name like `../vm/secrets`) would slip
/// past it and resolve into `/vm/` once the VFS normalizes the path.
fn check_mapping_target(to: &str) -> anyhow::Result<()> {
    if maps_into_vm(to) {
        return Err(make_malformed_runner_error(&format!(
            "mapping into /vm/ is forbidden: {to}"
        )));
    }
    if to.split('/').any(|component| component == "..") {
        return Err(make_malformed_runner_error(&format!(
            "mapping path with a '..' component is forbidden: {to}"
        )));
    }
    Ok(())
}

/// How a resolved runner id should be loaded into the archive cache.
pub(crate) enum ResolvedKind {
    /// A packaged `name:hash` runner read from the runners directory.
    Disk {
        name: symbol_table::GlobalSymbol,
        hash: Bytes32Hash,
    },
    /// `chain:<address>:<a|f>:<slot>` -- code blob read from a storage slot. The
    /// current contract (`contract`) also resolves here, pointing at its own
    /// address/state/`code_slot`.
    Chain {
        address: calldata::Address,
        on: runners::ChainState,
        slot: crate::SlotID,
    },
    /// `custom:<hash>` -- a runner registered at runtime. Carries no data: a
    /// `custom:` runner resolves against the VM's loaded set only, never a
    /// registry lookup, so the load action needs nothing but the canonical id.
    Custom,
}

/// A runner id resolved to its canonical cache key together with the way it
/// should be loaded.
pub(crate) struct Resolved {
    id: symbol_table::GlobalSymbol,
    kind: ResolvedKind,
}

impl From<runners::Id> for Resolved {
    fn from(id: runners::Id) -> Self {
        let kind = match &id {
            runners::Id::Builtin { name, hash } => ResolvedKind::Disk {
                name: name.clone(),
                hash: hash.clone(),
            },
            runners::Id::Chain { address, on, slot } => ResolvedKind::Chain {
                address: *address,
                on: *on,
                slot: *slot,
            },
            runners::Id::Custom { .. } => ResolvedKind::Custom,
        };
        Self {
            id: id.canonical(),
            kind,
        }
    }
}

/// Resolves a runner id to its canonical cache key and load strategy.
///
/// Free-standing so it can be shared between contract initialization
/// ([`Ctx`]) and runtime `gl_call`s ([`load_action`]).
pub(crate) async fn resolve_runner_id(
    supervisor: &rt::supervisor::Supervisor,
    topmost_runner_id: &runners::Id,
    id: &str,
) -> anyhow::Result<runners::Id> {
    let Some(parsed) = runners::parse_runner_id(id) else {
        return Err(make_malformed_runner_error(
            "runner id doesn't match expected format",
        ));
    };

    let resolved: runners::Id = match parsed {
        runners::IdUnresolved::Contract => topmost_runner_id.clone(),
        runners::IdUnresolved::Builtin { name, hash } => {
            let hash: Bytes32Hash = if hash == "test" || hash == "latest" {
                if !supervisor.shared_data.debug_mode.allows_latest_resolution() {
                    log_warn!(":{hash} runner used in non-debug mode, this is not allowed");
                    return Err(make_malformed_runner_error(
                        "runner id doesn't match expected format",
                    ));
                }
                let new_latest = supervisor.runner_cache.get_latest(&name);
                log_info!(runner_id = name.as_str(), tag = hash.as_str(); "resolving :test/:latest runner");
                let Some(new_latest) = new_latest else {
                    return Err(make_malformed_runner_error(
                        "runner id doesn't match expected format",
                    ));
                };
                Bytes32Hash::from_gvm32(new_latest).map_err(|e| {
                    make_malformed_runner_error(&format!(
                        "runner hash `{new_latest}` is not valid gvm32: {e}"
                    ))
                })?
            } else {
                Bytes32Hash::from_gvm32(&hash).map_err(|e| {
                    make_malformed_runner_error(&format!(
                        "runner hash `{hash}` is not valid gvm32: {e}"
                    ))
                })?
            };

            let hash_gvm32 = hash.to_gvm32();
            if !supervisor.runner_cache.has_in_all(&name, &hash_gvm32) {
                anyhow::bail!("runner {}:{} not found", name, hash_gvm32);
            }

            runners::Id::Builtin {
                name: symbol_table::GlobalSymbol::new(&name),
                hash,
            }
        }
        runners::IdUnresolved::Chain { address, on, slot } => {
            let on = on.unwrap_or(runners::ChainState::Accepted);
            let slot = match slot {
                Some(s) => s,
                None => {
                    let mode = on.host_storage_type().ok_or_else(|| {
                        make_malformed_runner_error(
                            "deploy-state chain runner cannot resolve its code slot from chain",
                        )
                    })?;
                    let mut storage = rt::vm::storage::Storage::new(
                        address,
                        supervisor.get_storage_limiter(),
                        crate::wasi::genlayer_sdk::StorageHostHolder(
                            supervisor.host.clone(),
                            crate::wasi::genlayer_sdk::ReadToken {
                                account: address,
                                mode,
                            },
                        ),
                    );
                    storage.resolve_code_slot().await.with_ctx(|| {
                        format!(
                            "resolving code slot for chain runner 0x{}",
                            address.checksum_hex_string()
                        )
                    })?
                }
            };
            runners::Id::Chain { address, on, slot }
        }
        runners::IdUnresolved::Custom { hash } => {
            let hash = Bytes32Hash::from_gvm32(&hash).map_err(|e| {
                make_malformed_runner_error(&format!(
                    "custom runner hash `{hash}` is not valid gvm32: {e}"
                ))
            })?;
            runners::Id::Custom { hash }
        }
    };

    Ok(resolved)
}

async fn resolve_runner(
    supervisor: &rt::supervisor::Supervisor,
    topmost_runner_id: &runners::Id,
    id: &str,
) -> anyhow::Result<Resolved> {
    Ok(resolve_runner_id(supervisor, topmost_runner_id, id)
        .await?
        .into())
}

/// Validates a `Sandbox`/`RunNondet` `custom_runners` grant list against the
/// parent's loaded set and returns the pins to grant the child.
///
/// - `list == None` -> grant every `custom:` runner in the parent's loaded set.
/// - `list == Some(list)` -> grant exactly `list`: every element must be a
///   `custom:<hash>` id, without duplicates, and present in the parent's loaded
///   set (i.e. the parent registered or was itself granted it).
///
/// `target` is the runner the child will execute; when it is a `custom:` id it
/// must be loaded in the parent and is auto-included (union) in the grant. Every
/// violation is the deterministic malformed-runner VM error. The returned pins
/// (Arc clones) keep the granted content alive across the parent's death, so a
/// queued nondet child still finds it; the child's spawn performs a load action
/// for each, charging its own limiter.
pub(crate) fn resolve_child_custom_runners(
    parent_loaded: &runners::cache::LoadedSet,
    list: Option<Vec<String>>,
    target: &runners::Id,
) -> anyhow::Result<Vec<runners::cache::ArchivePin>> {
    let mut granted_ids: HashSet<symbol_table::GlobalSymbol> = HashSet::new();
    let mut grants: Vec<runners::cache::ArchivePin> = match list {
        None => {
            let pins = parent_loaded.custom_pins();
            granted_ids.extend(pins.iter().map(|p| p.runner_id()));
            pins
        }
        Some(list) => {
            let mut grants = Vec::with_capacity(list.len());
            for elem in list {
                let hash = match runners::parse_runner_id(&elem) {
                    Some(runners::IdUnresolved::Custom { hash }) => hash,
                    _ => {
                        return Err(make_malformed_runner_error(&format!(
                            "custom_runners entry `{elem}` is not a `custom:` runner id"
                        )));
                    }
                };
                let hash = Bytes32Hash::from_gvm32(&hash).map_err(|e| {
                    make_malformed_runner_error(&format!(
                        "custom_runners entry `{elem}` hash is not valid gvm32: {e}"
                    ))
                })?;
                let id = runners::Id::Custom { hash }.canonical();
                let Some(pin) = parent_loaded.get(id) else {
                    return Err(make_malformed_runner_error(&format!(
                        "custom_runners entry `{elem}` is not loaded in this scope"
                    )));
                };
                if !granted_ids.insert(id) {
                    return Err(make_malformed_runner_error(&format!(
                        "custom_runners entry `{elem}` is duplicated"
                    )));
                }
                grants.push(pin.clone());
            }
            grants
        }
    };

    if let runners::Id::Custom { .. } = target {
        let id = target.canonical();
        let Some(pin) = parent_loaded.get(id) else {
            return Err(make_malformed_runner_error(
                "runner to execute is a custom runner not loaded in this scope",
            ));
        };
        if granted_ids.insert(id) {
            grants.push(pin.clone());
        }
    }

    // Every granted pin is drawn from the parent's loaded set.
    #[cfg(debug_assertions)]
    for pin in &grants {
        debug_assert!(
            parent_loaded.contains(pin.runner_id()),
            "granted custom runner {} absent from parent loaded set",
            pin.runner_id().as_str(),
        );
    }

    Ok(grants)
}

/// A charged det-mode load folds its canonical runner id into the execution
/// fingerprint; when `None` (nondet mode, or a free/cached load) nothing is
/// folded. The id is hashed to a fixed 32 bytes first, so
/// variable-length ids cannot alias each other (or a 32-byte sub-VM small-hash)
/// in the shared fingerprint stream.
fn fold_det_fingerprint(
    det_fingerprint: Option<&mut sha3::Sha3_256>,
    id: symbol_table::GlobalSymbol,
) {
    if let Some(fp) = det_fingerprint {
        use sha3::Digest as _;
        let id_hash: [u8; 32] = sha3::Sha3_256::digest(id.as_str().as_bytes()).into();
        fp.update(id_hash);
    }
}

/// The stable per-load-action observability line: operators and the
/// integration harness grep for the `runner load` message. `status` is
/// `charged` or `cached`. (`const` is a Rust keyword, hence `runner_load_cost`.)
fn log_runner_load(id: symbol_table::GlobalSymbol, size: u32, status: &'static str) {
    log_info!(
        runner = id.as_str(),
        runner_load_cost = public_abi::memory_limiter_consts::RUNNER_LOAD_COST,
        size = size,
        status = status;
        "runner load"
    );
}

async fn record_runner_load(
    supervisor: &rt::supervisor::Supervisor,
    id: symbol_table::GlobalSymbol,
    size: u32,
    status: &'static str,
) {
    supervisor
        .action_recorder
        .record_or_log(
            "runner_load",
            BTreeMap::from([
                ("runner_id".to_owned(), id.as_str().to_owned()),
                ("size".to_owned(), size.to_string()),
                ("status".to_owned(), status.to_owned()),
            ]),
        )
        .await;
}

fn out_of_memory() -> anyhow::Error {
    rt::errors::Error::vm(abi::consts::VmError::out_of().memory().val()).into()
}

/// Charges `RUNNER_LOAD_COST + size` to the VM's limiter, OOM on failure.
/// A `size` past `u32::MAX`, or a sum that overflows, cannot fit any budget and
/// maps to the same OOM.
fn charge_load(limiter: &rt::memlimiter::Limiter, size: usize) -> anyhow::Result<()> {
    let ok = u32::try_from(size)
        .ok()
        .and_then(|size| public_abi::memory_limiter_consts::RUNNER_LOAD_COST.checked_add(size))
        .is_some_and(|amount| limiter.consume(amount));
    if !ok {
        return Err(out_of_memory());
    }
    Ok(())
}

/// Records an already-charged load: folds the det fingerprint, inserts the pin
/// into the loaded set, logs the line. Split from [`charge_load`] because a
/// cache miss charges *before* materializing (so the charge precedes the peak),
/// then records once the pin exists.
fn record_charged_load(
    loaded: &mut runners::cache::LoadedSet,
    det_fingerprint: Option<&mut sha3::Sha3_256>,
    pin: runners::cache::ArchivePin,
) {
    let id = pin.runner_id();
    let size = pin.total_size();
    fold_det_fingerprint(det_fingerprint, id);
    loaded.insert(pin);
    log_runner_load(id, size, "charged");
}

/// Attaches to an already-materialized shared cell: charges `RUNNER_LOAD_COST + size`
/// then records the load. Its charge equals a miss's by content-determinism, so
/// a VM cannot observe whether it materialized or attached.
fn attach_load(
    limiter: &rt::memlimiter::Limiter,
    loaded: &mut runners::cache::LoadedSet,
    det_fingerprint: Option<&mut sha3::Sha3_256>,
    pin: runners::cache::ArchivePin,
) -> anyhow::Result<runners::cache::ArchivePin> {
    charge_load(limiter, pin.total_size() as usize)?;
    let out = pin.clone();
    record_charged_load(loaded, det_fingerprint, pin);
    Ok(out)
}

/// The **load action**: the single way any runner enters a VM.
///
/// 1. already in the VM's loaded set -> free, done;
/// 2. else charge `RUNNER_LOAD_COST + size` to `limiter` (OOM on failure),
///    materialize-or-attach the content, insert the pin, fold the det
///    fingerprint.
///
/// `custom:` runners resolve against the loaded set *only*: they enter it via
/// `RegisterRunner` or an inherited grant (both insert directly), so a `custom:`
/// id that is not already loaded here is a malformed-runner error -- never a
/// registry lookup.
pub(crate) async fn load_action(
    supervisor: &rt::supervisor::Supervisor,
    limiter: &rt::memlimiter::Limiter,
    loaded: &mut runners::cache::LoadedSet,
    det_fingerprint: Option<&mut sha3::Sha3_256>,
    resolved: Resolved,
) -> anyhow::Result<runners::cache::ArchivePin> {
    if let Some(pin) = loaded.get(resolved.id) {
        log_runner_load(resolved.id, pin.total_size(), "cached");
        record_runner_load(supervisor, resolved.id, pin.total_size(), "cached").await;
        return Ok(pin.clone());
    }

    let Resolved { id, kind } = resolved;
    match kind {
        ResolvedKind::Custom => Err(make_malformed_runner_error(&format!(
            "custom runner {id} is not registered in this execution scope"
        ))),
        ResolvedKind::Disk { name, hash } => {
            let cell = supervisor.runner_cache.cell(id);
            if cell.initialized() {
                let pin = attach_load(
                    limiter,
                    loaded,
                    det_fingerprint,
                    runners::cache::pin_of(cell),
                )?;
                record_runner_load(supervisor, id, pin.total_size(), "charged").await;
                return Ok(pin);
            }
            // Miss: learn the size from the file, charge, then materialize (so the
            // charge precedes the resident copy).
            let mut path = supervisor.runner_cache.runners_path().to_owned();
            runners::append_runner_subpath(name.as_str(), &hash.to_gvm32(), &mut path);
            path.set_extension("tar");
            if !path.exists() {
                return Err(rt::errors::Error::internal(format!("runner {id} not found")).into());
            }
            let data = util::mmap_file(&path)
                .with_ctx(|| format!("memory mapping runner archive for {id}"))?;
            let charged_size = data.as_ref().len();
            charge_load(limiter, charged_size)?;
            cell.get_or_try_init(|| async {
                let data = bytes::Bytes::copy_from_slice(data.as_ref());
                let arch = runners::Archive::from_ustar(data)
                    .with_ctx(|| format!("parsing ustar archive for {id}"))?;
                Ok::<_, rt::errors::Error>(runners::ArchiveCache::new(id, arch))
            })
            .await?;
            let pin = runners::cache::pin_of(cell);
            // Content-determinism: attach and miss charge the same size.
            debug_assert_eq!(
                pin.total_size() as usize,
                charged_size,
                "materialized disk archive size differs from the charged size"
            );
            record_charged_load(loaded, det_fingerprint, pin.clone());
            record_runner_load(supervisor, id, pin.total_size(), "charged").await;
            Ok(pin)
        }
        ResolvedKind::Chain { address, on, slot } => {
            let cell = supervisor.runner_cache.cell(id);
            if cell.initialized() {
                let pin = attach_load(
                    limiter,
                    loaded,
                    det_fingerprint,
                    runners::cache::pin_of(cell),
                )?;
                record_runner_load(supervisor, id, pin.total_size(), "charged").await;
                return Ok(pin);
            }
            let mode = on.host_storage_type().ok_or_else(|| {
                make_malformed_runner_error("deploy-state chain runner is not available on chain")
            })?;
            let mut storage = rt::vm::storage::Storage::new(
                address,
                supervisor.get_storage_limiter(),
                crate::wasi::genlayer_sdk::StorageHostHolder(
                    supervisor.host.clone(),
                    crate::wasi::genlayer_sdk::ReadToken {
                        account: address,
                        mode,
                    },
                ),
            );
            let dep_major = storage
                .read_major()
                .await
                .with_ctx(|| format!("reading major for chain runner {id}"))?;
            let node_major = genvm_common::version::CURRENT.major;
            if dep_major as u16 != node_major {
                return Err(rt::errors::Error::wrap(
                    public_abi::VmError::invalid_contract().major_mismatch(),
                    anyhow::anyhow!(
                        "chain runner {id} major {dep_major} != node major {node_major}"
                    ),
                )
                .into());
            }
            // Read the 4-byte length prefix, charge, then fetch the blob inside
            // the creator: a single `RUNNER_LOAD_COST + code_size` charge covers the peak
            // -- the old chain double-charge is gone.
            let code_size = storage
                .read_code_len(slot)
                .await
                .with_ctx(|| format!("reading chain runner code length for {id}"))?;
            charge_load(limiter, code_size as usize)?;
            cell.get_or_try_init(|| async move {
                let code = storage
                    .read_code_blob(slot, code_size)
                    .await
                    .with_ctx(|| format!("reading chain runner code for {id}"))?;
                let arch = runners::parse(bytes::Bytes::from(code))
                    .with_ctx(|| format!("parsing chain runner for {id}"))?;
                Ok::<_, rt::errors::Error>(runners::ArchiveCache::new(id, arch))
            })
            .await?;
            let pin = runners::cache::pin_of(cell);
            // Content-determinism: the charged prefix length must equal the
            // parsed archive's `total_size` (what a later attach will charge).
            debug_assert_eq!(
                pin.total_size(),
                code_size,
                "chain archive total_size differs from the charged code size"
            );
            record_charged_load(loaded, det_fingerprint, pin.clone());
            record_runner_load(supervisor, id, pin.total_size(), "charged").await;
            Ok(pin)
        }
    }
}

/// Inherit-at-spawn load action for a granted custom runner: the child already
/// holds the pin (carried in `SingleVMData`), so this attaches to it -- charging
/// the child's limiter and inserting into the child's loaded set.
/// Idempotent: a grant equal to the child's `custom:` entry point is loaded once.
pub(crate) fn inherit_load(
    limiter: &rt::memlimiter::Limiter,
    loaded: &mut runners::cache::LoadedSet,
    det_fingerprint: Option<&mut sha3::Sha3_256>,
    pin: runners::cache::ArchivePin,
) -> anyhow::Result<()> {
    if loaded.contains(pin.runner_id()) {
        log_runner_load(pin.runner_id(), pin.total_size(), "cached");
        return Ok(());
    }
    attach_load(limiter, loaded, det_fingerprint, pin)?;
    Ok(())
}

/// Registers `code` as a `custom:<hash>` runner via the load action against the
/// current VM. The error ladder (spec: `RegisterRunner` error
/// guarantees), in check order:
///
/// 1. already in this VM's loaded set -> free no-op, same id;
/// 2. charge `RUNNER_LOAD_COST + code.len()` -> OOM error, nothing charged or
///    registered;
/// 3. parse (attach to a live registry entry, or parse and insert) -> a parse
///    failure is a deterministic invalid-contract error; the charge is retained
///    (released with the VM like any charge) and the runner is not in the loaded
///    set, hence not resolvable;
/// 4. success -> pin inserted into the loaded set, canonical id returned.
///
/// The registered code length equals the archive `total_size`, so the charge
/// equals a later re-register's or an inherit's. A malformed `code` never enters
/// the weak registry (parse runs before insert), so the attach shortcut cannot
/// mask a parse error -- outcomes depend only on the bytes, never cache state.
pub(crate) async fn register_runner_load(
    supervisor: &rt::supervisor::Supervisor,
    limiter: &rt::memlimiter::Limiter,
    loaded: &mut runners::cache::LoadedSet,
    det_fingerprint: Option<&mut sha3::Sha3_256>,
    code: bytes::Bytes,
) -> anyhow::Result<symbol_table::GlobalSymbol> {
    let hash = runners::custom_runner_hash(&code);
    let id = runners::Id::Custom { hash }.canonical();
    let was_loaded = loaded.get(id).map(|pin| pin.total_size());
    let id = register_runner_load_into(
        &supervisor.custom_runners,
        limiter,
        loaded,
        det_fingerprint,
        code,
    )
    .await?;
    let (status, size) = match was_loaded {
        Some(size) => ("cached", size),
        None => (
            "charged",
            loaded.get(id).map(|pin| pin.total_size()).unwrap_or(0),
        ),
    };
    record_runner_load(supervisor, id, size, status).await;
    Ok(id)
}

/// Core of [`register_runner_load`], parameterized by the weak registry for
/// testability.
async fn register_runner_load_into(
    registry: &runners::cache::WeakCache,
    limiter: &rt::memlimiter::Limiter,
    loaded: &mut runners::cache::LoadedSet,
    det_fingerprint: Option<&mut sha3::Sha3_256>,
    code: bytes::Bytes,
) -> anyhow::Result<symbol_table::GlobalSymbol> {
    let hash = runners::custom_runner_hash(&code);
    let id = runners::Id::Custom { hash }.canonical();

    if let Some(pin) = loaded.get(id) {
        log_runner_load(id, pin.total_size(), "cached");
        return Ok(id);
    }

    // Charge before parsing (closes the previously-uncharged parse window).
    charge_load(limiter, code.len())?;

    let cell = registry.cell(id);
    cell.get_or_try_init(|| async {
        let archive = runners::parse(code).map_err(|e| {
            rt::errors::Error::wrap(public_abi::VmError::invalid_contract().val(), e)
        })?;
        Ok::<_, rt::errors::Error>(runners::ArchiveCache::new(id, archive))
    })
    .await?;

    record_charged_load(loaded, det_fingerprint, runners::cache::pin_of(cell));
    Ok(id)
}

/// Maps a file (or, when `file` ends with `/`, a directory subtree) from a
/// runner archive into the VM filesystem at `to`. Mirrors [`InitAction::MapFile`]
/// so the runtime `MapFile` `gl_call` behaves identically.
pub(crate) fn map_archive_file(
    preview1: &mut crate::wasi::preview1::Context,
    limiter: &rt::memlimiter::Limiter,
    arch: &runners::ArchiveCache,
    file: &str,
    to: &str,
) -> anyhow::Result<()> {
    if file.ends_with("/") {
        let is_root = file == "/";

        let range = if is_root {
            arch.files.data.range::<str, std::ops::RangeFull>(..)
        } else {
            arch.files.data.range(String::from(file)..)
        };

        let must_start_with: &str = if is_root { "" } else { file };

        for (name, file_contents) in range {
            if name.ends_with("/") {
                continue;
            }

            if !name.starts_with(must_start_with) {
                log_trace!(
                    from = file,
                    to = to,
                    name = name,
                    must_start_with = must_start_with;
                    "aborting file mapping"
                );
                break;
            }

            let mut name_in_fs = String::from(to);
            if !name_in_fs.ends_with("/") {
                name_in_fs.push('/');
            }
            name_in_fs.push_str(&name[must_start_with.len()..]);

            check_mapping_target(&name_in_fs)?;

            if !limiter
                .consume(public_abi::memory_limiter_consts::FILE_MAPPING + name_in_fs.len() as u32)
            {
                return Err(
                    rt::errors::Error::vm(abi::consts::VmError::out_of().memory().val()).into(),
                );
            }

            preview1.map_file(&name_in_fs, file_contents.clone())?;
        }
    } else {
        check_mapping_target(to)?;

        if !limiter.consume(public_abi::memory_limiter_consts::FILE_MAPPING + to.len() as u32) {
            return Err(
                rt::errors::Error::vm(abi::consts::VmError::out_of().memory().val()).into(),
            );
        }

        preview1.map_file(to, arch.get_file(file)?)?;
    }

    Ok(())
}

/// Maps one of an already-resolved `runner`'s files into `preview1` at runtime.
/// This keeps all runner-loading knowledge in this module: callers (e.g. the
/// `MapFile` `gl_call`) resolve the id and delegate, instead of stitching the
/// archive loading together themselves.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn map_runner_file(
    supervisor: &rt::supervisor::Supervisor,
    preview1: &mut crate::wasi::preview1::Context,
    limiter: &rt::memlimiter::Limiter,
    loaded: &mut runners::cache::LoadedSet,
    det_fingerprint: Option<&mut sha3::Sha3_256>,
    runner: runners::Id,
    path_in_runner: &str,
    path_in_vfs: &str,
) -> anyhow::Result<()> {
    // The load action pins the archive into the VM's loaded set, keeping its
    // backing bytes (which the mapped files share) resident for the VM's life.
    let arch = load_action(supervisor, limiter, loaded, det_fingerprint, runner.into()).await?;
    map_archive_file(preview1, limiter, &arch, path_in_runner, path_in_vfs)?;
    Ok(())
}

impl Ctx<'_, '_> {
    async fn resolve_runner(&self, id: &str) -> anyhow::Result<Resolved> {
        resolve_runner(self.supervisor, &self.topmost_runner_id, id).await
    }

    async fn get_arch(
        &mut self,
        resolved: Resolved,
    ) -> anyhow::Result<(symbol_table::GlobalSymbol, runners::cache::ArchivePin)> {
        // A `Depends`/`With` in a runner.json goes through the load action, so a
        // `custom:` dependency resolves against this VM's loaded set only --
        // a granted `custom:` runner cannot pull in a non-granted one.
        let id = resolved.id;
        let is_det = self.vm.config_copy.permissions.deterministic;
        let data = self.vm.store.data_mut();
        let limiter = data.limits.clone();
        let crate::wasi::genlayer_sdk::Context { loaded, data, .. } =
            &mut data.genlayer_ctx.genlayer_sdk;
        let det_fingerprint = is_det.then_some(&mut data.det_subvm_hashes);
        let pin = load_action(self.supervisor, &limiter, loaded, det_fingerprint, resolved).await?;
        Ok((id, pin))
    }

    fn load_modules(
        &mut self,
        current: symbol_table::GlobalSymbol,
        path: &std::sync::Arc<str>,
    ) -> anyhow::Result<Option<rt::DetNondet<wasmtime::Module>>> {
        let Some((id, hash)) = runners::verify_runner(current.as_str()) else {
            return Ok(None);
        };

        // `deserialize_file` below trusts the file as native code without looking
        // at the wasm it was built from, so only ids the precompiler can actually
        // have written may derive a path here. `precompile` only ever emits
        // artifacts for runners listed in `all.json`. Every other id must fall
        // through to a normal compile instead of picking up whatever happens to
        // sit at that path, notably `custom:<hash>`, which an attacker chooses
        // via RegisterRunner and which parses as a plain `name:hash` pair.
        if !self.supervisor.runner_cache.has_in_all(id, hash) {
            return Ok(None);
        }

        let special_name = caching::path_in_zip_to_hash(path);
        let Some(cache_dir) = &self.supervisor.wasm_mod_cache.cache_dir else {
            return Ok(None);
        };

        let mut cache_dir = cache_dir.to_owned();
        cache_dir.push(caching::PRECOMPILE_DIR_NAME);
        runners::append_runner_subpath(id, hash, &mut cache_dir);
        cache_dir.push(special_name);

        let det_mod = cache_dir.with_extension(caching::DET_NON_DET_PRECOMPILED_SUFFIX.det);

        if !det_mod.exists() {
            return Ok(None);
        }

        cache_dir.set_extension(caching::DET_NON_DET_PRECOMPILED_SUFFIX.non_det);
        let non_det_mod = cache_dir;

        if !non_det_mod.exists() {
            return Ok(None);
        }

        self.supervisor
            .shared_data
            .metrics
            .supervisor
            .precompile_hits
            .increment();

        Ok(Some(rt::DetNondet {
            det: unsafe {
                wasmtime::Module::deserialize_file(&self.supervisor.engines.det, &det_mod)
            }
            .with_context(|| format!("deserializing det module {path:?} of {current}"))?,
            non_det: unsafe {
                wasmtime::Module::deserialize_file(&self.supervisor.engines.non_det, &non_det_mod)
            }
            .with_context(|| format!("deserializing non-det module {path:?} of {current}"))?,
        }))
    }

    async fn link_wasm(
        &mut self,
        contents: bytes::Bytes,
        current: symbol_table::GlobalSymbol,
        path: &std::sync::Arc<str>,
    ) -> anyhow::Result<sync::DArc<rt::DetNondet<wasmtime::Module>>> {
        let mut wasm_key = String::from(current.as_str());
        wasm_key.push(':');
        wasm_key.push_str(path);

        let wasm_key = symbol_table::GlobalSymbol::from(wasm_key);

        let ret_mod = self
            .supervisor
            .wasm_mod_cache
            .wasm_modules_cache
            .get_or_create(wasm_key, || async {
                match self.load_modules(current, path) {
                    Ok(Some(loaded)) => return Ok(loaded),
                    Ok(None) => {}
                    Err(e) => {
                        log_error!(path:? = path, error:ah = e; "failed to load precompiled wasm module, recompiling");
                    }
                }

                self.supervisor
                    .compile_wasm(contents.as_ref(), wasm_key.as_str())
                    .await
                    .with_context(|| format!("compiling wasm {path:?} of {}", current))
            })
            .await?;

        Ok(ret_mod)
    }

    pub async fn apply(
        &mut self,
        action: &runners::InitAction,
        current: symbol_table::GlobalSymbol,
        current_runner_arch: &runners::ArchiveCache,
    ) -> anyhow::Result<Option<wasmtime::Instance>> {
        use runners::InitAction;

        #[derive(Clone)]
        enum RunnerArchive<'a> {
            Borrowed(&'a runners::ArchiveCache),
            Owned(runners::cache::ArchivePin),
        }

        impl<'a> std::ops::Deref for RunnerArchive<'a> {
            type Target = runners::ArchiveCache;

            fn deref(&self) -> &Self::Target {
                match self {
                    Self::Borrowed(arch) => arch,
                    Self::Owned(arch) => arch,
                }
            }
        }

        enum Work<'a> {
            SetState {
                current: symbol_table::GlobalSymbol,
                current_runner_arch: RunnerArchive<'a>,
                contexts: VecDeque<String>,
            },
            Action(InitAction),
        }

        let context_capture = self.supervisor.shared_data.debug_mode.capture();
        let next_context = |contexts: &VecDeque<String>, context: String| {
            next_action_context(context_capture, contexts, context)
        };

        let apply_contexts = |mut err: anyhow::Error, contexts: &VecDeque<String>| {
            for context in contexts.iter().rev() {
                err = err.context(context.clone());
            }
            err
        };
        macro_rules! try_context {
            ($expr:expr, $contexts:expr) => {
                match $expr {
                    Ok(v) => v,
                    Err(e) => return Err(apply_contexts(e.into(), $contexts)),
                }
            };
        }

        let mut current = current;
        let mut current_runner_arch = RunnerArchive::Borrowed(current_runner_arch);
        let mut contexts = VecDeque::new();
        let mut stack = vec![Work::Action(action.clone())];

        while let Some(work) = stack.pop() {
            let action = match work {
                Work::SetState {
                    current: next_current,
                    current_runner_arch: next_arch,
                    contexts: next_contexts,
                } => {
                    current = next_current;
                    current_runner_arch = next_arch;
                    contexts = next_contexts;
                    continue;
                }
                Work::Action(action) => action,
            };

            match action {
                InitAction::MapFile { to, file } => {
                    let limiter = self.vm.store.data_mut().limits.clone();
                    let preview1 = &mut self.vm.store.data_mut().genlayer_ctx_mut().preview1;
                    try_context!(
                        map_archive_file(preview1, &limiter, &current_runner_arch, &file, &to),
                        &contexts
                    );
                }
                InitAction::AddEnv { name, val } => {
                    let new_val = try_context!(
                        genvm_common::templater::patch_str(
                            &self.env,
                            &val,
                            &genvm_common::templater::DOLLAR_UNFOLDER_RE,
                        ),
                        &contexts
                    );
                    self.env.insert(name.clone(), new_val);
                }
                InitAction::SetArgs(args) => {
                    try_context!(
                        self.vm
                            .store
                            .data_mut()
                            .genlayer_ctx_mut()
                            .preview1
                            .set_args(&args[..]),
                        &contexts
                    );
                }
                InitAction::LinkWasm(path) => {
                    let contents = try_context!(
                        current_runner_arch
                            .get_file(&path)
                            .with_ctx(|| format!("getting file {path:?}")),
                        &contexts
                    );

                    let module = try_context!(
                        self.link_wasm(contents, current, &path)
                            .await
                            .with_context(|| format!("linking wasm {path:?}")),
                        &contexts
                    );
                    let module =
                        module.into_gep(|x| x.get(self.vm.config_copy.permissions.deterministic));

                    let instance = {
                        let instance = try_context!(
                            self.vm
                                .linker
                                .instantiate_async(&mut self.vm.store, &module)
                                .await
                                .with_context(|| format!("instantiating {path:?}")),
                            &contexts
                        );
                        let name = try_context!(
                            module
                                .name()
                                .ok_or_else(|| {
                                    anyhow::anyhow!("can't link unnamed module {:?}", current)
                                })
                                .with_context(|| {
                                    format!("getting module name for {path:?} of {current}")
                                })
                                .map_err(|e| {
                                    rt::errors::Error::wrap(
                                        public_abi::VmError::invalid_contract().wasm().linking(),
                                        e,
                                    )
                                }),
                            &contexts
                        );
                        try_context!(
                            self.vm
                                .linker
                                .instance(&mut self.vm.store, name, instance)
                                .with_context(|| {
                                    format!("linking instance {name} for {path:?}")
                                }),
                            &contexts
                        );
                        instance
                    };
                    match instance.get_typed_func::<(), ()>(&mut self.vm.store, "_initialize") {
                        Err(_) => {}
                        Ok(func) => {
                            log_info!(runner = current_runner_arch.runner_id().as_str(), path = path; "calling _initialize");
                            try_context!(func.call_async(&mut self.vm.store, ()).await, &contexts);
                        }
                    }
                }
                InitAction::StartWasm(path) => {
                    let env: Vec<(String, String)> = self
                        .env
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    try_context!(
                        self.vm
                            .store
                            .data_mut()
                            .genlayer_ctx_mut()
                            .preview1
                            .set_env(&env),
                        &contexts
                    );
                    let contents = try_context!(
                        current_runner_arch
                            .get_file(&path)
                            .with_ctx(|| format!("getting file {path:?}")),
                        &contexts
                    );
                    let module = try_context!(
                        self.link_wasm(contents, current, &path)
                            .await
                            .with_context(|| format!("linking wasm {path:?}")),
                        &contexts
                    );

                    let module =
                        module.into_gep(|x| x.get(self.vm.config_copy.permissions.deterministic));

                    return Ok(Some(try_context!(
                        self.vm
                            .linker
                            .instantiate_async(&mut self.vm.store, &module)
                            .await
                            .with_context(|| format!("instantiating {path:?}")),
                        &contexts
                    )));
                }
                InitAction::When { cond, action: next } => {
                    if (cond == runners::WasmMode::Det)
                        == self.vm.config_copy.permissions.deterministic
                    {
                        stack.push(Work::Action(*next));
                    }
                }
                InitAction::Seq(vec) => {
                    for act in vec.into_iter().rev() {
                        stack.push(Work::Action(act));
                    }
                }
                InitAction::With {
                    runner: uid,
                    action: next,
                } => {
                    let resolved = try_context!(self.resolve_runner(&uid).await, &contexts);
                    let (uid, new_arch) = try_context!(self.get_arch(resolved).await, &contexts);

                    let old_current = current;
                    let old_arch = current_runner_arch.clone();
                    let old_contexts = contexts.clone();

                    let next_contexts = next_context(&contexts, format!("With {uid}"));

                    stack.push(Work::SetState {
                        current: old_current,
                        current_runner_arch: old_arch,
                        contexts: old_contexts,
                    });
                    stack.push(Work::Action(*next));
                    stack.push(Work::SetState {
                        current: uid,
                        current_runner_arch: RunnerArchive::Owned(new_arch),
                        contexts: next_contexts,
                    });
                }
                InitAction::Depends(uid) => {
                    let resolved = try_context!(self.resolve_runner(&uid).await, &contexts);

                    if self.visited.insert(resolved.id) {
                        let uid = resolved.id.clone();
                        log_trace!(uid = uid; "adding dependency");

                        let (uid, new_arch) = try_context!(
                            self.get_arch(resolved)
                                .await
                                .with_context(|| format!("getting archive for {uid}")),
                            &contexts
                        );

                        let new_action = try_context!(
                            new_arch
                                .get_actions()
                                .await
                                .with_ctx(|| format!("loading {uid} runner.json")),
                            &contexts
                        );

                        let old_current = current;
                        let old_arch = current_runner_arch.clone();
                        let old_contexts = contexts.clone();

                        let next_contexts =
                            next_context(&contexts, format!("applying Depends {uid}"));

                        stack.push(Work::SetState {
                            current: old_current,
                            current_runner_arch: old_arch,
                            contexts: old_contexts,
                        });
                        stack.push(Work::Action((*new_action).clone()));
                        stack.push(Work::SetState {
                            current: uid,
                            current_runner_arch: RunnerArchive::Owned(new_arch),
                            contexts: next_contexts,
                        });
                    }
                }
            }
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
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
                "ctx-0", "...", "ctx-2", "ctx-3", "ctx-4", "ctx-5", "ctx-6", "ctx-7", "ctx-8",
                "ctx-9", "ctx-10", "ctx-11", "ctx-12", "ctx-13", "ctx-14", "ctx-15",
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
        let got =
            resolve_child_custom_runners(&parent, Some(vec![custom_id_str(1)]), &target).unwrap();
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
        let limiter =
            limiter_with_budget(public_abi::memory_limiter_consts::RUNNER_LOAD_COST + 100);
        charge_load(&limiter, 100).unwrap();
        assert_eq!(
            limiter.get_remaining_memory(),
            0,
            "charge must be exactly RUNNER_LOAD_COST + size"
        );
    }

    #[test]
    fn charge_load_oom_charges_nothing() {
        let budget = public_abi::memory_limiter_consts::RUNNER_LOAD_COST + 99;
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
        let budget = 2 * (public_abi::memory_limiter_consts::RUNNER_LOAD_COST + 1);
        let limiter = limiter_with_budget(budget);
        let mut loaded = runners::cache::LoadedSet::default();
        let granted = pin(custom_id(1));

        inherit_load(&limiter, &mut loaded, None, granted.clone()).unwrap();
        let after_first = limiter.get_remaining_memory();
        assert_eq!(
            budget - after_first,
            public_abi::memory_limiter_consts::RUNNER_LOAD_COST + 1
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
        let budget = public_abi::memory_limiter_consts::RUNNER_LOAD_COST;
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
        let budget = 2 * (public_abi::memory_limiter_consts::RUNNER_LOAD_COST + code.len() as u32);
        let limiter = limiter_with_budget(budget);
        let mut loaded = runners::cache::LoadedSet::default();

        let id = register_runner_load_into(&registry, &limiter, &mut loaded, None, code.clone())
            .await
            .unwrap();

        assert_eq!(id, custom_id_of(&code));
        assert_eq!(
            budget - limiter.get_remaining_memory(),
            public_abi::memory_limiter_consts::RUNNER_LOAD_COST + code.len() as u32
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
            let again =
                register_runner_load_into(&registry, &limiter, &mut loaded, None, code.clone())
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
        let budget = public_abi::memory_limiter_consts::RUNNER_LOAD_COST + code.len() as u32 - 1;
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
        let budget = public_abi::memory_limiter_consts::RUNNER_LOAD_COST + code.len() as u32;
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
        let id =
            register_runner_load_into(&registry, &parent_limiter, &mut parent, None, code.clone())
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
        let cost = public_abi::memory_limiter_consts::RUNNER_LOAD_COST + code.len() as u32;
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
        let cost = public_abi::memory_limiter_consts::RUNNER_LOAD_COST + code.len() as u32;
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
}
