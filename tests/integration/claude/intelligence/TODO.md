# TODO

## Harness notes

Superseded 2026-07-28. `run_test.py`, `run-manager.sh` and the `ya-test-runner`
workarounds are gone: `genvm-tool test run --filter-name <regex>` isolates a
single case and starts the manager itself. See the `agentic-fuzzing` skill.

## High Priority

- [ ] Explore executor host functions (`executor/src/host/`) for non-determinism vectors
- [x] Explore WASI implementation for filesystem/clock/random syscalls that could cause divergence (all deterministic - MT19937 rng, fixed clocks, FNV hash, deterministic memory)
- [ ] Test floating point operations across leader/validators for determinism
- [ ] Test memory allocation patterns that might differ across runs

## Medium Priority

- [ ] Test exception handling edge cases in contract calls
- [ ] Test storage read/write ordering under concurrent-like scenarios
- [ ] Test cross-contract call edge cases (recursion, reentrancy)

## Low Priority

- [ ] Test large contract deployments and memory limits
- [ ] Test calldata parsing edge cases (malformed JSON, unicode, etc.)
- [ ] Test gas/resource exhaustion boundaries
