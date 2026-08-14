# Explored Paths

## WASI Implementation (executor/src/wasi/)

### Files Explored
- `executor/src/wasi/preview1.rs` - All WASI Preview1 syscalls
- `executor/src/wasi/vfs.rs` - Virtual filesystem implementation
- `executor/src/wasi/genlayer_sdk.rs` - GenVM extension functions
- `executor/src/wasi/base.rs` - Config struct (is_deterministic, permissions)
- `executor/src/wasi/mod.rs` - WASI module initialization
- `executor/src/lib.rs` - Main executor, always sets is_deterministic=true
- `executor/src/rt/supervisor/mod.rs` - Wasmtime engine config (floats disabled, NaN canonicalization)

### Findings - All Deterministic
1. **`random_get`**: Uses MT19937 with hardcoded seed `[GenL, ayer]` in deterministic mode
2. **`clock_time_get`**: Returns fixed timestamp from blockchain message
3. **All clocks** (monotonic, perf_counter, etc.): Map to same fixed timestamp
4. **`process_time`**: Returns 0
5. **VFS**: Read-only, immutable content from pre-mapped Bytes
6. **`fd_readdir`**: BTreeMap ensures sorted order
7. **`environ_get`/`args_get`**: Pre-populated, fixed values
8. **Sockets**: All blocked (EACCES)
9. **Write ops**: All blocked (EROFS)
10. **`proc_exit`**: Deterministic

### Python Runtime Observations
- `PYTHONHASHSEED`: Not set, but Python uses FNV 32-bit hash (no randomization)
- `hash()` on strings/bytes: Deterministic across runs
- `set` iteration order: Deterministic (follows from hash determinism)
- `id()`/`repr()`: Memory addresses identical across runs (WASM deterministic memory)
- `os.getpid()`: Fixed at 42
- `sys.platform`: "wasi"
- Only 3 env vars: PYTHONHOME, PYTHONPATH, pwd

### Probes run (all pass, no divergence found)

The probes themselves are gone — they are throwaway now, and only this record
survives. `float_math` was the exception: it was promoted to
`tests/integration/nasty-determinism/floats`, reporting raw `struct.pack('<d')`
bits rather than `repr()`.

- `agent/wasi_random/` - os.urandom, random module
- `agent/wasi_clock/` - time.time, monotonic, perf_counter, clock_gettime
- `agent/hash_random/` - Python hash() function, PYTHONHASHSEED
- `agent/environ_args/` - os.environ, sys.argv, sys.path, os.getpid
- `agent/set_order/` - set/frozenset iteration order, dict ordering
- `agent/id_repr/` - id(), repr(), object.__hash__
- `agent/float_math/` - transcendental float math (sin/cos/exp/sqrt/log/atan2)
  + repr(); VERIFIED deterministic via `nix develop .#full` (l==v==s byte-identical,
  returns `453.77516336575655|3.141592653589793|1.4142135623730951|0.30000000000000004`)

> NOTE: finding 1 above ("hardcoded seed [GenL,ayer]") is STALE. `random_get` is
> now seeded from `sha3-256(stdin)` (8 LE u32 words) — see preview1.rs Context::new
> and spec 02-wasip1.rst. Determinism is preserved; the seed just varies per input.

## Spec audit (doc/website/src/spec vs executor)

Cross-checked the spec against the implementation. Constants in `constants.rst`
match `sdk-rs/src/abi/consts.rs` (both generated from `codegen/data/public-abi.json`).
Three mismatches found and fixed (committed):
1. `vm_error` codes: spec rendered `OOM:RAM`, wire format is space-joined `OOM RAM`
   (fixed in codegen template `rst.rb`; corroborated by `exit_code 1` in result hashes).
2. `fd_prestat_get`: spec said "Notsup otherwise"; impl returns `Prestat` for dirs,
   `Badf` for everything else.
3. `path_rename` was missing from the WASI Rofs always-erroring list.

## Recursion depth (2026-07-28)

Two throwaway probes, both ruled out; neither is a finding.

- `_scratch/calldata_deep/` — a list nested 20 000 deep handed to
  `gl.vm.UserError.immediate`, aiming at a recursive calldata decoder on the
  Rust side. Unreachable from a contract: `genlayer/calldata/__init__.py:204`
  recurses per level in Python, so CPython's own limit (~990 here) raises
  `RecursionError` long before the encoded value leaves the guest. Any depth a
  contract can express is far below what would trouble a native decoder. The
  Rust decoder would have to be probed from a malicious *host*, not a contract.
- `_scratch/deep_self_call/` — 256 levels of `gl.contract.get_at(self).view()`
  self-recursion, each level a nested VM instance on the host. Terminates in the
  canonical `out_of memory` (space-joined, from `rt/memlimiter.rs` — it is the
  RAM budget that runs out, not the native stack), identical across
  leader/validator/sync. Correct behaviour, not an internal error.

  Note for a follow-up: the contract cannot observe this. Wrapping the nested
  call in `try/except Exception` and returning the depth reached produced a
  byte-identical execution hash to the uncaught version — the limiter aborts the
  whole outer transaction, so the guest's handler never runs. How deep it got
  before dying was therefore not measured.

Both were run with `ignore-hash` flipped to `False` in
`executors/v0.3.x/.genvm-tool.py`, so the l/v/s hash comparison was live and
did agree.

## ADR-015 cross-major calls (PR #9, 2026-07-28)

Reviewed `resolve_callcontract_executor`/`run_nested` (ADR-015) against
`executor/src/wasi/genlayer_sdk/run.rs` (`gl_call_contract`,
`derive_call_contract_vm_data`, `run_nested_call_contract`, ~line 270-423) and
`implementation/src/manager/run.rs` `start_nested` (~1808-1929). Every
`NestedRunEnvelope` field (message with `value=0`/`is_init=false`, permissions
bitmap, `state_mode`, `topmost_runner_id`, `remaining_recursion`,
`remaining_det_fuel`, `memory_limit`) is built from the same
`derive_call_contract_vm_data` that already feeds the in-process route, so it
matches the spec table field-by-field; nothing in the path reads a HashMap in
iteration order, wall clock, or thread-scheduling-dependent state (the only
`SystemTime::now()` near the nested path is a `#[cfg(test)]` unix-socket-name
helper). No divergence hypothesis survived code review.

**This path is unreachable from a `_scratch` jsonnet probe.** The jsonnet
runner always constructs `MockHost` with
`resolve_callcontract_executor_hook=None`
(`tests/runner/genvm_tool_plugins/integration.py:685`), and `MockHost.
resolve_callcontract_executor` returns `None` whenever the hook is unset
(`tests/runner/gvm_extra/mock_host.py:139-151`) — so `call_contract_route`
always takes `CallContractRoute::InProcess` and `run_nested_call_contract` is
dead code from a jsonnet case's point of view, however the routing payload is
shaped. The hook is only wired up in the Python system-test harness (see
`tests/system/cross-major/test.py`'s `MultiConnectionMockHost`, which already
covers direction, signer forwarding, oversized blob vs `max_message_bytes`,
structured-error degradation to `major_mismatch`, and a busy/looping callee).
A future session hunting this feature should extend that system test, not
`_scratch` — jsonnet probes cannot reach it at all, and any that appear to
"pass" while doing so are exercising the unchanged in-process route, not the
nested one.

- `_scratch/recursion_budget/` — 100 levels of `gl.contract.get_at(self).view()`
  self-recursion (same shape as `deep_self_call`, smaller), aimed at
  `de59b7b`'s `depth: u32` → `remaining_recursion: u32` rename in
  `wasi/genlayer_sdk/{mod,run}.rs` and `rt/supervisor/mod.rs`. Diff-read as a
  1:1 semantic-preserving rewrite (`depth >= VM_RECURSION` before increment ⇔
  `remaining_recursion == 0` before `saturating_sub(1)`) and confirmed by
  running it: `genvm.log.gz` shows ~16 real `call_contract` spawns (16×
  `resolving :test/:latest runner` + matching `runner load` pairs) before
  `out_of memory` fires — same OOM-before-`VM_RECURSION`-limit shape
  `deep_self_call` already found, and the three `hash` artifacts are
  byte-identical (md5 `3162e5ab...`) across l/v/s. No divergence.

## Runner ids, custom-runner grants and storage state modes (PR #9, 2026-07-29)

Three probes over the non-cross-major half of the branch. One found an internal
error (kept, see below); the other two found nothing and are gone.

- `_scratch/runner_ids/` — **KEPT, it fails.** Sweeps 14 guest-supplied runner
  ids through `gl.vm.spawn_sandbox(runner=…)` and `gl.vm.map_file(runner, …)`,
  i.e. through `rt/supervisor/actions.rs::resolve_runner_id`. Twelve of the
  fourteen land on a canonical `invalid_contract runner malformed` /
  `invalid_contract runner absent` VMError or run fine (`contract`, `chain:<self>:a:<real
  slot>`, `chain:<self>:f`, `py-genlayer:test|latest`, `custom:` unregistered,
  `chain:<empty account>`, `chain:<self>:d`, zero slot, `''`, `@@@…`, a
  non-canonical gvm32 hash). The two that do not are any *well formed*
  `name:<canonical gvm32>` whose pair is not installed —
  `py-genlayer:8b8kjyda2ycxyq4ea6g4yfpnydxhd52gqba5rb8dw7krkh5mn9p0` and
  `nosuchrunner:9b8kjyda…p0`: `resolve_runner_id` reaches
  `anyhow::bail!("runner {}:{} not found")` (actions.rs:177), a plain anyhow
  error rather than an `Error::wrap(VmError…)`, so `Error::trap` turns it into
  **INTERNAL_ERROR**. Severity 4, contract-controlled, both entry points. **Not
  a regression of this PR**: the bail, the `Sandbox`/`MapFile` call sites and
  the `Error::trap(anyhow_to_wasmtime(e))` mapping are byte-identical at the
  base gitlink `acb37c7` (`git show acb37c7:executor/src/rt/supervisor/
  actions.rs`, lines 142-143 and `genlayer_sdk.rs` 1919-1921 / 1676-1677). The
  sibling `Error::internal(format!("runner {id} not found"))` at actions.rs:467
  is the same defect one layer down, unreachable only because the bail fires
  first. Both belong behind `make_malformed_runner_error`.

- `_scratch/custom_runners/` — deleted, nothing. `resolve_child_custom_runners`
  (new on this branch) via a raw `gl_call_generic({'Sandbox': …})` carrying a
  hand-built `custom_runners` grant list: `None`, `[]`, the parent's own
  registered id, a duplicate pair, a non-`custom:` id, an unregistered
  `custom:`, `@@@`, and an absent `name:hash`. Every violation is the canonical
  `invalid_contract runner malformed` — note the absent `name:hash` is rejected
  *here* rather than reaching the bail above, because the grant list is checked
  for the `custom:` prefix before any registry lookup. `register_runner` with
  six garbage archives (empty, plain text, `#`, non-json header, a header
  depending on an absent registry runner, one depending on an unregistered
  `custom:`) returns either a `custom:` id or `invalid_contract runner absent`. l/v/s
  hashes agreed on all 49 runs.

- `_scratch/state_modes/` — deleted, nothing. Aimed at the working-tree
  `ChainState::for_vm` fix in `lib.rs` and at `gl_call_contract`'s new
  `state == Default → inherit the parent's state_mode` (base forced
  `LatestNonFinal`). Four call shapes (plain nested call, local write before the
  nested call, two nested calls in a row, nested call inside a sandbox) × the
  three `StorageType` values, plus a top-level step with `is_init: true` and
  `code: null` to force `ChainState::for_vm(true, …) == Deploy`. All 45 runs
  agree l/v/s, and the deploy-state top-level run ends in the canonical
  `invalid_contract runner malformed` (the `d` chain state has no
  `host_storage_type`, so the cache miss cannot be filled) — correct, not an
  internal error. **The three storage modes are indistinguishable under the
  jsonnet harness**: `gvm_extra/mock_host.py::MockHost.storage_read` ignores its
  `mode` argument entirely, so `LatestFinal`, `LatestNonFinal` and `Default` all
  read the same bytes. A finalized-vs-accepted divergence cannot be found from a
  jsonnet case at all; it needs a host that keeps two snapshots.

All three ran with `ignore-hash` flipped to `False` in
`executors/v0.3.x/.genvm-tool.py`, so the l/v/s comparison was live.

## Balance-funded fees, raw storage, backtraces and sub-VM folding (PR #9, 2026-07-29)

Five probes. One found an internal error (kept); four found nothing and are gone.

### What the jsonnet harness actually compares

Worth knowing before writing another probe: the `hash` artifact a jsonnet case
compares across l/v/s is **not** the executor's `execution_hash`. It is a tuple
the harness builds itself in `tests/runner/genvm_tool_plugins/integration.py:850`
— `[result_kind, result_data, result_fingerprint, result_storage_changes,
result_events]`. The executor's `execution_hash` also folds `subvm_hashes`,
`data_fees_consumed` and `data_fees_remaining`
(`executor/src/host/mod.rs::FullResult::new`), and **none of those three are in
the compared tuple**, so a divergence confined to them passes a jsonnet case
silently even with `ignore-hash: False`. The real hash is reachable, though: it
is the `execution_hash` attribute of every step's `result.pickle` artifact.
Reading it out of the three modes' pickles (`pickletools.genops`, take the bytes
after the `execution_hash` key — the pickle needs the harness venv to unpickle
properly) is a strictly stronger oracle and is what the two folding probes below
were actually checked against.

### FIXED: `_scratch/fee_zero_budget/`

Resolved by `9d2db0d`, which gave the fee expression language `true`/`false`
literals (`expr/{tokenizer,lexer,evaluator,value}.rs`). The `else false` arm now
parses, so a zero per-round budget evaluates instead of raising `undefined
variable`. The account below is the original report, kept because the reachability
notes at its end are still accurate.


A balance-funded internal message (`use_balance=True`) whose guest-supplied
`execution_budget_per_round` is **0** ends the whole transaction in
INTERNAL_ERROR, in all three modes:

    genvm-tool test run --filter-name 'fee_zero_budget'
    # 0_0_0   budget=1 -> "emitted"
    # 0_0_0_0 budget=0 -> internal error, l/v/s alike
    # genvm.log.gz:
    #   rt::fees "failed to evaluate fee expression"
    #     error: internal error: undefined variable `false`
    #   genvm "internal error": calculating message fee internal:
    #     failed to evaluate fee expression: undefined variable `false`

Cause: the `message_fee` `delta_expr` in `executor/install/config/genvm.yaml`
(line ~186) reads

    let budgetTooLow =
      if a.matchedFeeParams.executionBudgetPerRound > 0
      then a.matchedFeeParams.executionBudgetPerRound < budgetFloor
      else false in

The fee expression language has no boolean **literals** — `Value::Bool` is only
ever produced by a comparison operator or `hasKey`
(`executor/crates/common/src/expr/`), so `false` is an unbound identifier. The
`else` arm is dead for a non-zero budget and is the only thing a zero budget
evaluates, so the branch is exactly the trigger. `rt::fees::eval` maps a
non-`ScriptVMError` `EvalError` to `Error::internal`, so the guest gets
INTERNAL_ERROR where a `VMError` belongs. The same `delta_expr` serves
`DeployContract`, so `gl.contract.deploy(..., use_balance=True)` with a zero
per-round budget is the same defect.

**This is this PR's regression.** The base gitlink `acb37c7` has no
`budgetTooLow`, no `messageBudgetFloor` and no `balanceFunded` argument — its
`messageFeeFloor` takes two arguments (`git show
acb37c7:executor/install/config/genvm.yaml`), and `git show
acb37c7:executor/src/wasi/genlayer_sdk.rs | grep -c use_balance` is 0: the whole
balance-funded path arrived on this branch. Severity 4.

Reaching the path from a jsonnet case at all is worth recording:
`can_use_balance_for_message_fees` is read from the root permission bitfield
(`lib.rs:349`), which a contract sets in its own `__init__` via
`gl.storage.Root.get().set_permission(Permissions.CAN_USE_BALANCE_FOR_MESSAGE_FEES,
True)` — after deployment the root slot is frozen by `Root.lock_default()`, so
the constructor is the only window.

Also note when writing such a case: **jsonnet numbers are IEEE doubles**, so a
literal `79228162514264337593543950335` (2^96-1) silently becomes 2^96 and the
executor rejects it as out of range. A magnitude-boundary probe has to build
big integers in the contract, not in the jsonnet.

### Deleted, nothing found

- `_scratch/stor_edge/` — raw `_genlayer_wasi.storage_read/storage_write` with
  contract-chosen slot ids, offsets and lengths, against the page arithmetic in
  `rt/vm/storage.rs`. `index + len` overflowing u32 is `Inval`; zero-length ops
  at offset 2^32-1 are no-ops; an unaligned 70-byte write spanning three pages
  reads back exactly; scattered far-apart pages and adjacent ones both survive
  `make_delta`'s run coalescing (the `res.last_mut().expect(...)` there is safe
  because `StoragePagesOverride` is an `rpds::RedBlackTreeMap` keyed by
  `PageID(SlotID, u32)`, so a page's predecessor is always already emitted).
  **Root-slot poisoning is closed**: every write to `SlotID::ZERO` — major,
  `code_slot` pointer, permission bitfield — returns `forbidden`, because the
  py-SDK's `Root.lock_default()` puts the root slot in `locked_slots` at deploy
  time. So the "point `code_slot` at a slot whose 4-byte length prefix is
  0xFFFFFFFF and let the runner load action allocate it" idea is unreachable
  from a deployed contract (and `charge_load` maps that size to OOM anyway).
  One harness caveat: a write at a *large* offset makes `MockHost` materialize a
  flat slot of that size — offset 4294967263 produced 4 GiB storage pickles and
  a `ConnectionTimeoutError`. That is the mock host, not the VM.

- `_scratch/bt_hash/` — the wasm backtrace is folded into the top-level
  `execution_hash` (`host/mod.rs`, key `backtrace`) whenever
  `needs_error_fingerprint` is set, which is true for the root VM and for an
  in-process `CallContract` child but false for `Sandbox` and `RunNondet`
  children. Drove the guest into `wasm_trap stack_overflow` with python's own
  recursion guard lifted (`sys.setrecursionlimit(10**8)`), from several
  different prior frame depths, inside a sandbox, and inside a `try/except`
  (which cannot catch it — the trap kills the VM). All three modes agree
  byte-for-byte on the executor `execution_hash`. Note `extract_backtrace` logs
  "no backtrace attached" for these traps, so the frame list is empty in
  practice and the fingerprint the in-process `CallContract` child computes is
  never folded anywhere: `RunResult::small_hash` hashes `kind`, `result`,
  `subvm_hashes` and `wasm_store_hashes`, but **not** `backtrace`.

- `_scratch/fold_mix/` — sub-VM folding and fee consumption, checked against the
  `result.pickle` `execution_hash` oracle rather than the harness tuple: 1 and 8
  sequential sandboxes, 6 levels of nested sandboxes, `register_runner` inside a
  sandbox (twice, same and differing code), register-then-`spawn_sandbox` on the
  registered `custom:` id, `emit_raw` with 0/1/4 topics and 0/7/1000-byte blobs,
  sandboxes interleaved with events, and a 4-deep in-process `CallContract`
  chain between two contracts, alone and mixed with sandboxes and an event. 14
  steps, all with distinct hashes, all three modes identical.

- `_scratch/fee_params/` — the wider fee-param sweep the minimal
  `fee_zero_budget` case was cut down from. Rotations of length 0/1/9/10/64,
  counts at 2^32-1, prices at the 2^96 bound and one bit over, on-acceptance on
  and off. Everything except the zero-budget case lands on `Errno::Inval` from
  `validate_balance_fee` or on the yaml's `vmError "fee too_many_rounds"`; the
  magnitude bound documented at `message.rs:225` (worst case < 2^183) holds, so
  `rational_to_u256`'s "exceeds U256 range" `internal_ensure!` is not reachable
  by making the numbers big — only by making the budget zero.

All five ran with `ignore-hash` flipped to `False` in
`executors/v0.3.x/.genvm-tool.py`, so the l/v/s comparison was live.

## Cross-major observability system test (2026-07-29)

Added `tests/system/cross-major-observability/test.py` in the manager root and
collected it from the root `.genvm-tool.py`. This extends the real-manager
`tests/system/cross-major` harness rather than adding a jsonnet case.

Not because a jsonnet case cannot reach `run_nested` -- it can. A case declares
a top-level `executor_routes` map of callee address to major, which makes the
plugin install a `resolve_callcontract_executor_hook` and set
`hook_cross_contract_calls` on the request
(`tests/runner/genvm_tool_plugins/integration.py`); `misc/routed_call` is such
a case.

Nor is it the oracle. An earlier note in this file says a jsonnet case compares
a tuple the harness builds itself rather than the executor's `execution_hash`.
**That is stale**: since `a5bad97` the plugin sets `hash_data =
res.execution_hash`, so a jsonnet case compares exactly what consensus does.

The actual reason is that a jsonnet case can set `permissions`, but there is no
way to inject a recursion budget or a host fuel value per step --
`unsafe_overrides` carries only `reroute_to`. This case needed both.

It *used* to also be true that the line compared no hashes at all: the setting
was `ignore-hash: True`, and it suppressed the leader-vs-validator comparison
along with the committed goldens. It is now `save-hashes: False`, which drops
only the goldens; a non-main mode is compared against the main mode of the same
run instead. Flipping the old flag by hand is no longer necessary, and the first
run under the new semantics found a case that had been silently broken --
`exploit/disagree_in_sandbox` declared a `leader_nondet` result its contract
could never consume, so its validator failed `leader_output extra` while the
leader returned normally.

Worth knowing either way: `execution_hash` folds eight fields (`backtrace`,
`data`, `data_fees_consumed`, `data_fees_remaining`, `kind`, `storage_changes`,
`subvm_hashes`, `wasm_store_hashes`) and **emissions are not among them**. Hash
equality across l/v/s does not by itself prove the three runs emitted the same
messages; in a jsonnet case that is covered separately by the `messages`
semantics component, which only runs in the main mode.

The case deploys hash-pinned v0.2/v0.3 contracts and compares the executor's
actual 32-byte `execution_hash` across genuine leader, validator and sync runs.
A deliberately unused leader result first proves the validator request is not
silently running as sync. Covered:

- v0.2 -> v0.3 preserved message fields (callee address, inherited
  sender/origin/signer, caller stack, zero value, non-init);
- a nested callee observing `6: forbidden` for raw storage write, message send,
  and `run_nondet`, while runner registration and a further deterministic
  contract call remain available when inherited from the parent;
- forced disabled debug (`py-genlayer:test` resolves under the unsafe top-level
  control run, but nested execution reports `invalid_contract runner
  malformed`);
- a root run with only `c` permission (therefore no `LockedSlotsSet`) reading
  the same nested stored value as `wscn`;
- host-routed self-calls with injected recursion budgets 0, 1, 2, 4, 5 and 6
  (0/1/2/4 produce canonical `out_of vm_recursion`; 5/6 finish);
- alternating v0.3/v0.2 chains at depths 1, 4 and 8;
- undeployed and non-contract addresses, a Python trap, and a `UserError`;
- host fuel values 0, 1, 1000 and 2^32.

No leader/validator/sync hash disagreement and no contract-triggered
`INTERNAL_ERROR` occurred.

Ruled out:

- **Nested `no_modules` can be distinguished from the stripped nondet
  permission by a contract:** it cannot. `run_nondet` is rejected as
  `Forbidden` before module access, and the manager rejects any nested envelope
  asserting `SPAWN_NONDET`. The callee-visible behavior is covered, but the two
  internal reasons are intentionally observationally equivalent.
- **Changing `remaining_fuel_as_gen` changes deterministic nested execution:**
  it did not for module-free work. All four host-fuel values returned the same
  value and the same hash (`d9320ff0...d975`) in l/v/s. The imported budget can
  only be spent/observed by module work, which the nested permission model makes
  unreachable.
- **Recursion exhaustion can be caught by the Python caller:** it cannot; the
  executor trap aborts the current VM and surfaces canonical `out_of
  vm_recursion`. It was byte-identical in all three modes at every tested
  budget.
- **An unused synthetic leader-result blob proves validator mode on either
  executor line:** only on v0.3. The same guard on a v0.2 root returned normally
  because that line tolerates unused leader results; moving the guard to a
  v0.3-root call produced the expected VMError. This was a test-oracle
  difference, not an l/v/s disagreement.

## Error-classification audit of the whole executor (2026-08-07)

A three-way static sweep of every file under `executor/src/` and
`executor/crates/`, looking for sites that report `ErrorKind::Internal` for a
failure the contract's own input decides. One defect survived triage.

### PROMOTED: `fd_seek` negation overflow

`preview1.rs` computed `-offset as u64` for a negative `Whence::Cur` seek.
`-i64::MIN` overflows, and both cargo profiles set `panic = "abort"`, so the
executor aborted:

    thread 'main' panicked at src/wasi/preview1.rs:830:46:
    attempt to negate with overflow

reported to the harness as `reason: 'internal error'`, identically in l/v/s. Any
contract that can open a mapped file reaches it — `os.lseek(fd, -(2**63),
os.SEEK_CUR)`. Fixed with `offset.unsigned_abs()`, which clamps to 0 like every
other over-large negative offset. Pinned by
`tests/integration/wasi/fd_seek_extremes/`, verified to fail before the fix.

### Ruled out

- **The rest of `wasi/`**: fd-table, path, iovec and buffer arithmetic is guarded
  by the `MAX_FDS` and `u32` limiter invariants; the remaining downcasts hold
  because `pos <= contents.len() <= u32::MAX` is maintained on every seek branch.
- **`message_fee_allocation` fee-expression failures** (`fees.rs:98,102`,
  `message.rs:74,190`): a negative or above-`U256` fee does reach
  `Error::internal`, but the allocation tree comes from the host's
  `ExecutionData` envelope (`lib.rs:427`), never from the contract. The contract
  only selects which node matches. Host input, so `Internal` is the correct
  classification.
- **Entry-message timestamp** (`preview1.rs:283`): `datetime.timestamp() as u64 *
  1_000_000_000` overflows for far-future dates, but the datetime is host-supplied
  too.
- **`runners/`, `exe/`, `host/`, `calldata`**: no input-determined `Internal`
  sites left. Archive, `runner.json`, version, template and environment failures
  all carry `invalid_contract runner malformed`; the calldata decoder's `expect`s
  guard states that arbitrary wire bytes cannot construct.
- **Memory-exhaustion paths** throughout: excluded by the determinism rule, since
  the outcome depends on pressure rather than on the input alone.

## Internal-error hunt: `?`-to-Internal conversions (2026-08-10)

Static sweep for contract-reachable `ErrorKind::Internal`: every `?` on a
foreign error whose blanket `From` maps to `Internal` (`io`, `serde_json`,
`Utf8Error`, `TryFromIntError`, `ZipError`, `BinaryReaderError`), every bare
`anyhow` `?`, and every `internal!`/`Error::internal`, across `wasi/`, `rt/`,
`runners/`, `host/`, `exe/`.

### PROMOTED & FIXED: locked-slots / upgraders read leaks a VMError as a crash

`create_supervisor` (`executor/src/lib.rs`) reads the sender's locked-slots and
upgraders set out of the *contract's own* root storage, whose length words the
contract controls. An over-limit `upgraders` (>32), `locked_slots` (>256), or a
memory budget exhausted mid-read returns a canonical `VmError::out_of(..)` from
`host/mod.rs`. `create_supervisor` returns `anyhow::Result` and this error was
`?`-propagated out of `exe::run::handle` -> `main` with no result frame, so the
harness saw `reason: 'internal error'`, identically in l/v/s.

Trigger: a contract grows its own `upgraders` VLA to 33 entries, then any later
write-permitted run aborts before its body runs. Fixed by catching the VM-kind
error in `create_supervisor` and deferring it to the run's prepare stage (the
existing VMError->receipt path in `run_with_impl`), so it comes back as
`VMError("out_of upgraders")`. Genuine host-read failures still stay `Internal`.
Pinned by `tests/integration/storage/upgraders_overflow/`, verified to fail
(internal error) before the fix.

Also fixed in passing: `rt/fees.rs:103` had a `log_error!` with malformed
key syntax (`error:rt::errors::internal!(...)`) that did not compile -- HEAD of
the v0.3 line did not build.

### Ruled out (with probes)

- **`register_runner` + `spawn_runner` + `map_file` over 30 malformed archives**
  (probe, now deleted): bad/truncated/non-zip, empty/non-json/non-utf8/deeply
  nested `runner.json`, garbage `chain:`/`custom:`/`name:` ids in `Depends`,
  `MapFile` with `..`, malformed wasm, text edge cases, local-vs-central header
  clashes, and `Depends` on a well-formed-but-absent runner id. Every one
  resolved to a canonical `invalid_contract runner malformed` / `wasm
  validating` / `wasm entrypoint`. No internal error, no panic.
- **200k randomly mutated runner zips** through `runners::parse` (throwaway Rust
  test, now deleted): never panicked.
- The `?`-to-`Internal` sites in `wasi/` and `rt/` are neutralized by local
  `From` impls (`TryFromIntError`/`serde_json` -> `Errno` on the WASI surface;
  leader-result parse is total via `malformed_leader_result`) or by explicit
  `Error::wrap(invalid_contract().., ..)`. `runners/`/`host/` `?` sites are
  either `map_err`'d to VM errors or fed by host/manifest input (correctly
  `Internal`). Two defence-in-depth gaps noted but not live: the main-runner
  `load_action` (`supervisor/mod.rs:571`) and the runtime-runner gl_calls lack a
  wrap barrier, benign only because their callees already return VM errors.

## Panic hunt: contract-reachable slice/arithmetic panics (2026-08-10)

Static sweep for panicking operations whose operands a contract chooses --
range slicing (`&x[a..b]`), unchecked `+`/`-`/`*` on `usize`/`u32`/`U256`,
`unwrap`/`expect`/`unreachable!`/`debug_assert!` -- across `wasi/`, `rt/`,
`runners/`, `host/`, `exe/` and `crates/{calldata,common,sdk-rs}`. One defect
found, kept as a failing probe.

### FOUND: `fd_pread` past EOF slices out of range

`_scratch/pread_oob/` -- **KEPT, it fails.** `fd_pread` (`preview1.rs:687`)
computes `buf_len = min(iov.buf_len, contents.len().saturating_sub(offset))` and
then slices `&contents[offset..offset + buf_len]`. The saturation only clamps
the *length*: for `offset > contents.len()` the length is 0 but the slice
*start* is still past the end, and `&v[50..50]` on a 49-byte slice panics.
`panic = "abort"` in both profiles, so the executor dies:

    thread 'main' panicked at src/wasi/preview1.rs:687:67:
    range start index 50 out of range for slice of length 49

reported to the harness as `reason: 'internal error'`. Unlike the `fd_seek`
negation overflow, this is a slice bounds check, so it is *not* debug-profile
dependent -- a release executor panics identically.

Trigger, from any contract that can map a file (probe registers its own runner):
`os.pread(fd, 8, len + 1)`. `os.pread(fd, 8, len)` is fine -- exactly at EOF the
start is in range. The equivalent `fd_read`/`fd_seek` paths are clamped
correctly; `fd_pwrite` is `Notsup`.

**Not a regression of any branch**: the expression is byte-identical at
`acb37c7`, the initial executor split.

### Ruled out

- **`fd_write`'s `size: u32` iovec accumulator** (`preview1.rs:725`) does
  overflow if one call's `ciov` lengths sum above 4 GiB (entries may alias the
  same guest memory, so the sum is not bounded by memory size), but it is
  unreachable from Python: the WASI CPython build has no `os.writev`/`os.readv`
  (probe printed `writev False`, `readv False`, `pread True`), so every guest
  write carries exactly one iovec, and reaching the bound would first write
  ~4 GiB to stdout.
- **`fees.rs` U256 arithmetic**: `CostVec::sum` and `consume_bucket_raw`'s
  `cumulative += prev_cost` are unchecked `U256` adds (the `uint` crate panics on
  overflow in *both* profiles), but every summand is checked `<= remaining`
  first, so overflowing needs bucket totals above 2^255 -- host input, not the
  contract's. `rational_to_u256` returning `U256::MAX` instead of erroring is
  what would feed it; the contract-facing magnitude bound (`message.rs:225`,
  worst case < 2^183) keeps it out of reach.
- **`rt/vm/storage.rs`** page arithmetic: `read`/`write` are bounded by the
  `index.checked_add(buf_len)` guard in `genlayer_sdk/mod.rs:653,720`, so every
  `page_idx * 32` stays inside `u32`; `make_delta`'s `k.1 - 1` is guarded by
  `k.1 != 0`.
- **`vfs::Trie::follow`'s `debug_assert!(is_normalized_path_component(..))`**:
  every caller normalizes with `absolute = true`, under which a `..` can never
  survive into the result, so the assert cannot fire on a guest path.
- **`runners/archive.rs`, `runners/parse.rs`, `crates/calldata` decoder,
  `crates/common/{gvm32,templater,version,logger,util/str}`**: all slicing is
  either `checked_add` + `bytes.get(..)`, guarded by a `starts_with`, or over
  fixed-size ASCII prefixes; the calldata uleb/container decoder bounds capacity
  by the remaining input.
- **`random_get`** bounds-checks the guest pointer before allocating, so its
  host-side `Vec` is bounded by guest memory.

## Internal-error hunt: the WASI/VFS surface a mapped file opens (2026-08-10)

Three contract-triggered `INTERNAL_ERROR`s, all reproduced in leader, validator
and sync alike, all present at the base commit `acb37c7` (none is a regression
of this branch). Probes kept under `_scratch/`, one per defect. Every one is
reached from a plain deterministic contract with no special permissions: the
entry ticket is `register_runner` + `map_file`, which any contract may call.

### `_scratch/pread_oob/` -- `fd_pread` slices from an offset past the end

    os.pread(fd, 4, len(file) + 1)

    thread 'main' panicked at src/wasi/preview1.rs:687:67:
    range start index 44 out of range for slice of length 43

`fd_pread` clamps the *length* (`contents.len().saturating_sub(offset)`) but
never the *start*: `&contents.as_ref()[offset..(offset + buf_len)]` panics
whenever `offset > contents.len()`, even though `buf_len` has been clamped to 0.
`offset == len` is fine, `offset == len + 1` aborts. `panic = "abort"` in both
profiles, so this is a process abort, reported as `reason: 'internal error'`.
Exactly the sibling of the `fd_seek` negation overflow fixed on 2026-08-07 --
the same audit read this function and stopped at the `try_into`s.

### `_scratch/vfs_deep_path/` -- deep VFS trie overflows the native stack

    map_file(rid, 'file', '/' + '/'.join('d' * 20000) + '/f.txt')

    thread 'main' has overflowed its stack
    fatal runtime error: stack overflow, aborting

`preview1::map_file` builds the directory trie iteratively, so the mapping
itself succeeds ("mapped" is printed); the crash comes afterwards, when the
20 000-deep `FilesTrie`/`BTreeMap` chain is dropped -- `Drop` is recursive and
there is no depth bound on a mapping target. 5 000 components survive, 20 000
does not, so the threshold sits in between and is stack-size dependent. The
memory limiter charges `FILE_MAPPING + path_len`, which a 40 KB path pays
easily; the limiter is not a depth bound.

### `_scratch/map_target_empty/` -- a VMError escapes as an internal error

    map_file(rid, 'file', '/')

    genvm "internal error": causes ["VMError(invalid_contract runner malformed)"]

Any mapping target that normalizes to zero components -- `''`, `'/'`, `'///'`,
`'/.'`, `'.'` -- hits the `locs_arr.is_empty()` guard in `preview1::map_file`,
which returns a perfectly canonical `invalid_contract runner malformed`. It
still comes back as `INTERNAL_ERROR` rather than as a receipt: the error is
raised as a `wasmtime::Result`, and by the time `lib.rs:550` looks at it the
`rt::errors::Error` downcast no longer succeeds, so `unwrap_vm_errors` treats it
as an executor fault. The same VMError raised elsewhere on the runner path (e.g.
`runner/custom_malformed`) does produce a receipt, so the defect is the route,
not the code. Targets that normalize to something non-empty (`/./a`, `/a//b`,
`/vmx/a`, `/a\0b`) all map fine.

### Ruled out in the same sweep

- **`fd_seek` extremes** (`2^63-1` and `-2^63` on Set/Cur, `End`): clamped or
  `Notsup`, as the 2026-08-07 fix intended.
- **Fd exhaustion**: 2 000 `open`s of a mapped file hit the `MAX_FDS` VMError
  cleanly and the freed descriptors are reused without a double release.
- **Long single path component** (1 MB): mapped and charged, no overflow.
- **`fd_write` iovec-sum overflow** -- PROMOTED, it is real. `size += add_size`
  in `fd_write` accumulates the ciovec lengths in a `u32`, and the buffers may
  overlap, so the sum is not bounded by the guest's memory. Not reachable from a
  Python contract (this CPython build has no `os.writev`/`os.readv`/`os.preadv`;
  probed), so the case is a hand-written `.wat` runner:
  `tests/integration/wasi/fd_write_iovec_overflow/`. It grows memory to the
  limiter's ceiling (63 487 pages here, ~3.875 GiB), then writes two ciovecs of
  `2^32 - total` and `total` bytes:

      thread 'main' panicked at src/wasi/preview1.rs:725:13:
      attempt to add with overflow

  The first ciovec is written before the second is added in, so an unfixed
  executor writes `2^32 - memory` (~128 MiB here) to stdout before aborting; the
  case refuses to run if that would exceed 256 MiB. Once the total is validated
  before anything is written, the case writes nothing but its marker.

### Promoted

All four now live in `tests/integration/wasi/` and all four are red until the
executor is fixed: `fd_pread_extremes`, `map_file_deep_path` (golden encodes a
refusal: a depth bound on a mapping target), `map_file_empty_target` (golden
encodes the receipt the route currently swallows) and `fd_write_iovec_overflow`.
New tags `wasi-fd-pread`, `wasi-fd-write` and `wasi-map-file` in
`tests/tags.json`.

## Internal-error hunt round 2: nothing new (2026-08-10)

A follow-up sweep after the four WASI/VFS defects. **No new contract-triggered
`INTERNAL_ERROR` found.** Recorded so the next session skips these.

### Statically re-read and ruled out

- **`rt/fees.rs` + `install/config/genvm.yaml`**: the only guest-controlled fee
  inputs are `calldataLength`/`blobSize`/`outputLength`/`pages` (linear, `ceilDiv`
  only -- `rational_to_u256`'s is-integer `internal_ensure!` cannot fire) and the
  `use_balance` `fee_params`. The rotations-length overrun the SDK deliberately
  does *not* bound is caught by the yaml's `if 2 * appealRounds >= arrayLen
  validatorsPerRound then vmError "fee too_many_rounds"`; `arrayGetElem rotations
  (idiv round 2)` and the `leaderRounds` fold both index within `len-1`.
  `CostVec::sum` is dead code (no non-test caller); `reported_fee`'s `self.0[0]`
  is guarded by `internal_ensure!(n > 0)` in `build_bucket`.
- **`runners/actions.rs` `InitAction` nesting**: serde_json's own 128-frame
  `check_recursion!` covers `deserialize_enum`/`_map`/`_seq`, and each nesting
  level of `Seq`/`When`/`With` costs two frames, so a runner.json cannot get
  past ~64 levels -- `validate_impl`'s `INIT_ACTION_DEPTH` (128) is a second
  belt. `Ctx::apply` is an explicit `Work` stack, not recursion, and `Depends`
  cycles are cut by `visited`. A `Depends` cycle between two *registered*
  runners is impossible by construction: `custom:<hash>` is the sha3 of the code,
  so A cannot name B while B names A.
- **`calldata`**: every contract-facing entry (`gl_call`'s `calldata::decode`,
  `decode_obj`'s `BinaryDeserializer`) caps depth at 128 with an explicit stack,
  and `from_value` then works on an already-bounded in-memory `Value`. The
  `Maybe` boundary *does* reset the depth budget (documented in
  `codec/de/unparsed.rs`), and `to_value`'s `expect("encode-decode roundtrip
  failed")` would abort if a re-encoded structure came out deeper than 128 --
  but a contract cannot reach it: everything it sends is decoded through the
  128-cap first and materialized (a `Value` source never produces
  `Maybe::Checked`), so only a malicious *host* could stack the budgets.
  `Maybe::kind()`'s `raw.0[0]` / `panic!("checked value is invalid ...")` has no
  caller in the executor at all.
- **`rt/memlimiter.rs`**: `release_no_consumed`'s post-hoc overflow `assert!`
  needs a release without a matching consume; `VFS::place_content`/`pop_fd` are
  symmetric on the `release_memory` flag, and mapping/fd charges are never
  released.
- **`rt/vm/storage.rs` `read_code_len`/`read_code_blob`**: a
  `chain:<addr>:<a|f>:<slot>` runner id lets a contract choose the 4-byte length
  word (any non-root slot is writable). The u32 page arithmetic in `Storage::read`
  *would* overflow, but only for `code_size >= u32::MAX - 2`, and `charge_load`
  maps exactly that range to `out_of memory` (`RUNNER_LOAD_COST + size` overflows
  the checked add). Smaller sizes read zeros and die in `runners::parse` as
  `invalid_contract runner absent`. `read` provably fills every byte of the
  `Box::new_uninit_slice` it hands out: the host-read window spans the first to
  the last unknown page, and the `else if` in the copy loop only gates *caching*.
- **`host/mod.rs::get_locked_slots_for_sender`**: `4 + i * Address::SIZE` is
  bounded by `UPGRADERS = 32`; the same for `LOCKED_SLOTS = 256`.
- **`crates/common/{templater,version,gvm32}`, `runners/{archive,parse}.rs`,
  `caching.rs`**: no unguarded slicing or arithmetic left.
- **`run.rs::strip_vm_error_detail`'s `debug_assert!(is_valid_(..))`**: the only
  free-form `VmError` string in the executor comes from a *nested executor's*
  reply (`ResultCode::VmError` -> `Cow::Owned(code)`), i.e. host input.
  `proc_exit` clamps to `exit_code 0..=125`, which `is_valid_` accepts.

### Probed and ruled out

- `_scratch/wasi_battery/` (deleted) -- ~45 `os.*` calls against a mapped file, a
  mapped subtree, the preopen root and fds 0/1/2/3: seeks and preads at
  `2**63-1`, reads at EOF, double `close`, `close(0)`/`close(2)` followed by a
  reopen that *reuses fd 0 and 2*, `openat` escaping via `../..`, `stat` on a
  100 000-char path and a 300-component path, `readdir` on a file, `read`/`pread`
  on a directory fd, and every write-side call. Every one returns a plain errno
  (`Rofs`/`Badf`/`Notsup`/`Isdir`/`Noent`) or succeeds. Note `os.posix_fallocate`,
  `os.statvfs`, `os.readv`/`writev` do not exist in this CPython build.
- `_scratch/maptraps/` (deleted) -- seven one-shot runs, one per `map_file`
  destination that traps: file over an existing dir, file under an existing leaf,
  a subtree under an existing leaf, `..` traversal, `/vm/`, whole archive onto
  `/`, and the component-count boundary. All canonical (`invalid_contract`,
  `invalid_contract runner malformed`), l/v/s identical. This also pins the new
  `VFS_PATH_COMPONENTS` bound: 128 components map, 129 are refused.

### Fuzzed in-process (new, and the fastest way to redo this)

The jsonnet harness cannot reach most raw WASI arguments -- CPython forms one
iovec per call, no `writev`/`readv`, and every pointer it passes is valid. A
`.wat` contract can, but hand-writing one per hypothesis is slow. A **unit test
inside `preview1.rs`** reaches the same surface with no wasm at all, because
`ContextVFS { vfs, context }` has crate-private fields and
`wiggle::GuestMemory::Unshared(&mut [u8])` is an ordinary byte slice:

```rust
let mut ctx = Context::new(chrono::DateTime::from_timestamp(0, 0).unwrap(), conf, [7u8; 32]);
ctx.map_file("/d/f.txt", bytes::Bytes::from_static(b"0123456789")).unwrap();
let mut vfs = vfs::VFS::new(Vec::new(), rt::memlimiter::Limiter::new()).unwrap();
let mut mem = vec![0u8; 1 << 16];
let mut c = ContextVFS { vfs: &mut vfs, context: &mut ctx };
let mut m = wiggle::GuestMemory::Unshared(&mut mem);
c.fd_pread(&mut m, fd, wiggle::GuestPtr::<[types::Iovec]>::new((0, 4000)), u64::MAX)
```

`futures` is not a dependency -- block on the async entry points with a
`tokio::runtime::Builder::new_current_thread()`.

**3 000 000 randomized calls** across every preview1 entry point
(`fd_read`/`fd_pread`/`fd_write` with up to 4096 iovecs of arbitrary
pointer/length words, `fd_readdir` at `cookie = u64::MAX` and
`buf_len = u32::MAX`, `fd_seek` at `i64::MIN`/`i64::MAX` on all three whences,
`random_get`/`args_get`/`environ_get`/`fd_prestat_dir_name` at the memory edge,
`path_open`/`path_filestat_get`/`path_readlink` over a path pool including `""`,
`"//////"`, `"../../.."`, an embedded NUL and `U+10FFFF`, plus `fd_close` /
`fd_renumber` / `fd_advise` / `fd_allocate` / `fd_filestat_set_size` and
`map_file` remapping the trie *under* the open fds) produced **no panic**. The
probe is deleted; rebuild it from the snippet above if a new hypothesis needs it.

Also checked: `calldata` decode -> encode -> decode over random byte strings is
byte-faithful and never panics (but random bytes are a poor generator -- ~1 % of
inputs decode; a grammar-based generator is needed to push this further, and
`crates/sdk-rs/fuzz/gvm-gl-call-roundtrip.rs` already covers `gl_call::Message`
under AFL).

### `rt/vm/storage.rs` fuzzed in-process too

`tests/storage_page_accounting.rs` already carries a `FakeHost`/`data_fees`
scaffold; reusing it, 60 000 randomized `read`/`write` pairs over offsets
clustered on page boundaries and on the `u32` ceiling
(`0, 1, 4, 31, 32, 33, 63, 64, 127, 1024, 0xffff, 0xfffff, MAX-64, MAX-33,
MAX-32, MAX-1` x lengths `0..70000`), interleaved with `fork`/`fold`, asserted
read-your-writes on every write. No panic, no stale read, no accounting
underflow. Probe deleted.

The one `Storage::read` call whose length is *not* bounded by the WASI
`index.checked_add(buf_len)` guard is `read_code_blob(slot, code_size)`, which
reads at `index = 4`. Its page arithmetic overflows `u32` for
`code_size >= u32::MAX - 2` (then `end_page * 32 == 2^32`), and that is
genuinely unreachable rather than merely unlikely: `charge_load` computes
`RUNNER_LOAD_COST + code_size` with a checked add, so it refuses everything
above `u32::MAX - 4096 == 4294963199 < 4294967293`. `write_code`'s explicit
`code.len() > u32::MAX - 4` rejection is the matching guard on the write side.
Do not re-derive this -- the margin is 4093 bytes and it is deliberate.

### The throughput unlock: wrap trapping probes in `spawn_sandbox`

Most `VMError`s are fatal traps, so the obvious probe shape is one contract per
case and one run per contract -- which is why earlier sweeps cost seven runs for
seven `map_file` destinations. That is unnecessary. **A sub-VM failure is a
value, not a trap, in the parent**, so one run can cover a whole corpus:

```python
res = spawn_sandbox(lambda: fn(arg), allow_write_storage=True, allow_send_messages=True)
print(label, str(res)[:110])   # Return(...) | VMError("...") | UserError(...)
```

Three things worth knowing before using it:

- `gl_call` **errno** failures raise a catchable Python `SystemError: <n>: <name>`
  in whatever VM makes the call, so those need no sandbox. Only the trap-shaped
  failures (`register_runner`, `map_file`, runner resolution) do.
- Grant the sandbox the permissions the case needs. A default sandbox has
  `write_storage=False`/`send_messages=False`, so `EmitEvent`, `PostMessage` and
  `RunNondet` come back `6: forbidden` and never reach their handlers.
- A malformed `gl_call` payload is rejected by the *decoder* long before the
  handler, and the payload shapes are easy to get wrong: the method name in a
  `MainCallData` object is the key `''` (not `'method'`), an address must be a
  `gl.Address` (raw `bytes` encodes as `TYPE_BYTES` and yields `2: inval`),
  `on` is `'accepted'`/`'finalized'`, `state` is the `StorageType` **int**, and
  unit variants (`Yield`, `GetTimestamp`) encode as bare strings, not
  single-key maps. If every case returns `2: inval`, the schema is wrong and
  nothing under test has run.

### Swept with it, all canonical, nothing found

- **44 adversarial `runner.json` documents x `register_runner` + `spawn_runner`**
  -- empty/duplicate-key/`$schema` objects, `StartWasm`/`LinkWasm` at `""`, `/`,
  `../file`, a non-wasm file and an absent file, `Depends` on `contract`/`""`/
  `custom:`/a bare `chain:` address, every `MapFile` destination shape,
  `AddEnv` with an empty name, a `=` in the name, a self-referential `${a}`,
  `${ENV[PATH]}` and a 100 000-byte value, `SetArgs`, `When` on both cond
  arms, `With` on `contract` and on an absent id, and `Seq`/`When` nested 40 and
  30 deep. Plus 15 raw archives (empty, `#`, `//`, `--`, bare version line,
  wasm magic with and without a trailing byte, `PK\x03\x04`, a bare EOCD,
  invalid UTF-8, trailing NULs).
- **17 VFS destinations x {file, file twice, whole subtree}** -- confirms the
  new bound from the inside: 127 components map, 128 map, 129 are refused,
  5000 are refused. `/contract.py` and `/py` are mappable (the modules are
  already imported, so it does nothing); mapping a subtree under an existing
  leaf is `invalid_contract`.
- **19 hand-built `gl_call` byte payloads + 45 structured messages** -- nesting
  at depth 2/127/128/129/1000/100000, huge ulebs, container lengths of 2^32-1,
  duplicate and descending map keys, an unknown variant, two variants in one
  map; `EthSend`/`EthCall` calldata of 0/1/3/4/5 bytes (the `call_key.0[..4]`
  boundary), `DeployContract` at `salt_nonce`/`value` = 2^256-1 and a 1 MiB
  code blob, `EmitEvent` with 0/1/4/5 topics and 31/32/33/0-byte topics and a
  256 KiB blob, method names of 0/31/32/33/100000 bytes (the `CallKey::
  for_method` hash boundary), and every unit variant.
- **The `chain:<addr>:<a|f>:<slot>` runner id, properly** -- an earlier attempt
  looked clean for the wrong reason: a chain runner reads through a *fresh*
  `Storage` bound to an explicit state mode, so it never sees the current
  transaction's uncommitted override. It has to be a two-step
  `util.chain([...])` case: step 0 writes the blob, step 1 resolves the id.
  Done that way, 13 blobs x both states behave: a valid text runner actually
  starts (`exit_code 1` from the empty entry payload), a declared length of
  `u32::MAX` and `u32::MAX - 3` are refused as `out_of memory` by `charge_load`
  **before** any allocation or host read, a short declaration is `invalid_contract
  runner malformed`, `0xff` bytes are `not_utf8_text`. Keep declared lengths
  under ~1 MiB: at 2 GiB the *mock host* materializes the slot and the case
  times out, which is a harness failure, not a finding.

### Concurrency over the shared runner cache -- ruled out

The one lead the static sweeps could not settle. A contract registered **24**
`custom:` runners (so the det VM pins 24 cells), then launched **48**
`run_nondet` blocks, each of which spawns a sandbox inside itself. In
validator/sync the leader result is consumed immediately and the real nondet VMs
are queued, so with `threads: 2` several run at once -- every one of them
attaching to all 24 granted pins through `inherit_load` -> `attach_load` ->
`LoadedSet::insert`, while also racing on the shared disk-runner cells
(`cpython`, `py-genlayer`, ...). Re-registering the same 24 archives afterwards
returned identical ids (all cache hits, pins still live).

All three modes printed the same thing and nothing fired: not
`LoadedSet::insert`'s re-insert assert, not `WeakCache::assert_invariants`, not
`assert_empty_on_teardown`, not `pin_of`'s initialized-cell assert. `dashmap`'s
per-shard `entry()` makes `WeakCache::cell` atomic, and `loaded` is a `&mut`
borrow, so no `await` can interleave two loads of the same set.

Two SDK gotchas that cost runs here: the leader closure is **cloudpickled**, so
it must not capture anything unpicklable (`gl.vm.VMError` fails with
`Can't pickle R: attribute lookup R on typing failed`) -- hoist the body into a
module-level function; and inside a contract method `run_nondet` already returns
the value, not a `Lazy`, so `.get()` raises `AttributeError`.

### `map_archive_file` -- ruled out

The last untested combination: archive entry names x `file`/`to` pairs, which is
where the only unchecked `u32` add on a contract-controlled length lives
(`FILE_MAPPING + name_len`, `actions.rs`), next to `&name[must_start_with.len()..]`
and `check_mapping_target`. Driven in-process (an in-crate `#[cfg(test)]` module,
since `map_archive_file` is `pub(crate)`; build an `Archive { data, total_size }`
literal and a `preview1::Context::new`), **320 000 calls** over 22 entry names
(including `a` vs `a/` vs `a0` ordering neighbours, multi-byte `\u{e9}` and
`\u{10ffff}` prefixes) x 16 `file` prefixes x 18 destinations, plus 1 MiB
destinations. No panic.

Both risky expressions are safe for structural reasons worth recording:
`&name[file.len()..]` is guarded by `name.starts_with(file)`, and a `str` prefix
match is always a char boundary; and `FILE_MAPPING + name_len` needs
`name_in_fs.len()` within 256 of `u32::MAX`, but `name_in_fs` is
`to + "/" + suffix` where `to` is bounded by guest memory (3.875 GiB max) and a
zip entry name is bounded by the `u16` name-length field (64 KiB), so the sum
cannot approach 2^32.

### Where a next session should look

Everything reachable from a *contract* through `wasi/`, `runners/`, `rt/fees`,
`rt/vm/storage` and `calldata` has now been swept twice. The remaining
`ErrorKind::Internal` and panic sites are all fed by host or node-config input
(fee-expression parse, allocation tree, module replies, nested-executor result
codes, `all.json` vs on-disk runner mismatch). Making progress there means
changing the threat model to a hostile host -- which is the AFL/system-test
lane, not a jsonnet probe.
