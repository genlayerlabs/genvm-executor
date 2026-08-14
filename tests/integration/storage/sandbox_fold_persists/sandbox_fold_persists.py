# { "Depends": "py-genlayer:test" }

import _genlayer_wasi as wasi
import genlayer as gl

SLOT = b'\x42' * 32
VALUE = b'A' * 32 + b'B' * 32 + b'C' * 32


class Contract(gl.contract.Contract):
	def __init__(self):
		def write_pages():
			wasi.storage_write(SLOT, 0, VALUE)

		gl.vm.spawn_sandbox(write_pages, allow_write_storage=True)

		readback = bytearray(len(VALUE))
		wasi.storage_read(SLOT, 0, readback)
		print(bytes(readback))
