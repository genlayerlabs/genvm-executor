# { "Depends": "py-genlayer:test" }

import _genlayer_wasi as wasi
import genlayer as gl

MIB = 1024**2
BALLAST = 3 * 1024 * MIB
BALLAST_CHUNK = 64 * MIB
PAYLOAD = 256 * MIB
SLOT = b'\x42' * 32


class Contract(gl.contract.Contract):
	def __init__(self):
		# Both prints must appear before the error, otherwise the run died in an
		# allocation and says nothing about storage.
		ballast = [bytearray(BALLAST_CHUNK) for _ in range(BALLAST // BALLAST_CHUNK)]
		print(f'ballast: {sum(map(len, ballast))}')

		payload = b'\x01' * PAYLOAD
		print(f'payload: {len(payload)}')

		wasi.storage_write(SLOT, 0, payload)
		print('write completed')
