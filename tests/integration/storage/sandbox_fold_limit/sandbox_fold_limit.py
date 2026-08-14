# { "Depends": "py-genlayer:test" }

import _genlayer_wasi as wasi
import genlayer as gl

MIB = 1024**2
BALLAST = 3 * 1024 * MIB
BALLAST_CHUNK = 64 * MIB
CHUNK_SIZE = 16 * MIB
SLOT = b'\x42' * 32
# Without the fold transfer the caller's budget never moves and this loop has no
# bound at all, so cap it: a regression must fail the golden, not hang the suite.
MAX_ITERATIONS = 32


class Contract(gl.contract.Contract):
	def __init__(self):
		# Leaves roughly a gigabyte, so a handful of handovers exhausts it.
		ballast = [bytearray(BALLAST_CHUNK) for _ in range(BALLAST // BALLAST_CHUNK)]
		print(f'ballast: {sum(map(len, ballast))}')

		completed = 0
		while completed < MAX_ITERATIONS:
			offset = completed * CHUNK_SIZE

			def write_chunk():
				wasi.storage_write(SLOT, offset, b'\x01' * CHUNK_SIZE)

			result = gl.vm.spawn_sandbox(write_chunk, allow_write_storage=True)
			if isinstance(result, gl.vm.VMError):
				print(f'completed iterations: {completed}')
				print(f'sandbox error: {result}')
				break
			completed += 1
		else:
			print(f'unbounded: {completed} handovers without running out of memory')
