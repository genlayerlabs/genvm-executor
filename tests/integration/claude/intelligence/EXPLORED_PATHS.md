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
  fourteen land on a canonical `invalid_contract malformed_runner` /
  `absent_runner_comment` VMError or run fine (`contract`, `chain:<self>:a:<real
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
  `invalid_contract malformed_runner` — note the absent `name:hash` is rejected
  *here* rather than reaching the bail above, because the grant list is checked
  for the `custom:` prefix before any registry lookup. `register_runner` with
  six garbage archives (empty, plain text, `#`, non-json header, a header
  depending on an absent registry runner, one depending on an unregistered
  `custom:`) returns either a `custom:` id or `absent_runner_comment`. l/v/s
  hashes agreed on all 49 runs.

- `_scratch/state_modes/` — deleted, nothing. Aimed at the working-tree
  `ChainState::for_vm` fix in `lib.rs` and at `gl_call_contract`'s new
  `state == Default → inherit the parent's state_mode` (base forced
  `LatestNonFinal`). Four call shapes (plain nested call, local write before the
  nested call, two nested calls in a row, nested call inside a sandbox) × the
  three `StorageType` values, plus a top-level step with `is_init: true` and
  `code: null` to force `ChainState::for_vm(true, …) == Deploy`. All 45 runs
  agree l/v/s, and the deploy-state top-level run ends in the canonical
  `invalid_contract malformed_runner` (the `d` chain state has no
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

### KEPT, it fails: `_scratch/fee_zero_budget/`

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
  control run, but nested execution reports `invalid_contract
  malformed_runner`);
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
