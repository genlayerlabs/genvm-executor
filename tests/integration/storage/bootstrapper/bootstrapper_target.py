# { "Depends": "py-genlayer:test" }

import genlayer as gl
from genlayer.types import i32


class Contract(gl.contract.Contract):
	field: i32

	@gl.public.write
	def get_field(self):
		print(int(self.field))
