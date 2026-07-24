# { "Depends": "py-genlayer:test" }
from genlayer.vm import register_runner

# Register ladder, malformed path: `RegisterRunner` charges
# `RUNNER_LOAD_COST + len(code)` *before* parsing, so a parse failure is a
# deterministic invalid-contract error with the charge retained (released with
# the VM) and the runner never resolvable. Observable here as: exactly one
# `charged` custom record (the earlier valid registration) and none for the
# malformed bytes — the failed parse emits no `runner load` record at all.
good = b'# { "Depends": "py-genlayer:test" }\nexit(0)\n'
register_runner(good)

# Not zip, not core-wasm, not valid UTF-8 text -> parse fails on the bytes alone.
register_runner(b'\xff\xfe\xfd')

print('unreachable: malformed register must raise a deterministic VM error')
