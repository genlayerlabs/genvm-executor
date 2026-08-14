# { "Depends": "py-genlayer:test" }

import genlayer as gl
from genlayer.types import Address


class Contract(gl.contract.Contract):
	def __init__(self, count: int):
		# Grow the contract's own `upgraders` root VLA past its limit (32). The
		# length word lives in ordinary storage, so this is contract-controlled.
		root = gl.storage.Root.get()
		root.upgraders.get().extend(
			Address(bytes([i & 0xFF]) + b'\x00' * 19) for i in range(count)
		)
		print(f'upgraders len={len(root.upgraders.get())}', flush=True)

	@gl.public.write
	def nop(self):
		# The next write-permitted run reads the over-limit set while assembling
		# the supervisor. That is a contract fault (`out_of upgraders`), and must
		# come back as a receipt -- not abort the executor with an internal error.
		print('nop body ran', flush=True)
