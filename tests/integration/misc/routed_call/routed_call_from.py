# { "Depends": "py-genlayer:test" }
import genlayer as gl
from genlayer.types import Address


class Contract(gl.contract.Contract):
	def __init__(self):
		pass

	@gl.public.write
	def main(self, addr: Address):
		print('routed_call from.main')
		print(gl.contract.get_at(addr).view().answer())
