# { "Depends": "py-genlayer:test" }

import _genlayer_wasi as wasi
import genlayer as gl

SLOT = b'\x42' * 32
CHUNK = 16 << 20
# One byte past the node's 64 MiB cap on a single host reply
SIZE = (64 << 20) + 1
MARKS = [0, CHUNK - 1, CHUNK, 2 * CHUNK + 7, SIZE - 1]


class Contract(gl.contract.Contract):
	def __init__(self):
		for i, off in enumerate(MARKS):
			wasi.storage_write(SLOT, off, bytes([i + 1]))

	@gl.public.view
	def read(self):
		buf = bytearray(SIZE)
		wasi.storage_read(SLOT, 0, buf)
		print('marks', [buf[off] for off in MARKS])
		print('non-zero bytes', len(buf) - buf.count(0))
