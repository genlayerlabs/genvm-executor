# { "Depends": "py-genlayer:test" }
from genlayer.vm import register_runner

# ADR-012 §3 register ladder, success path: the first registration charges
# `RUNNER_LOAD_COST + len(code)` and emits one `charged` custom record; a second
# registration of the *same* code in the *same* VM is a free no-op that resolves
# to the same id and emits one `cached` custom record.
code = b'# { "Depends": "py-genlayer:test" }\nexit(0)\n'

id1 = register_runner(code)
id2 = register_runner(code)

print('id', id1)
print('idempotent', id1 == id2)
exit(0)
