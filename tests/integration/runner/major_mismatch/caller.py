# { "Depends": "py-genlayer:test" }
import genlayer as gl
from genlayer.types import Address


class Contract(gl.contract.Contract):
	def __init__(self):
		pass

	@gl.public.write
	def call(self, addr: Address):
		gl.contract.get_at(addr).view().foo()

	@gl.public.write
	def map(self, addr: Address):
		gl.vm.map_file(gl.vm.RunnerIDOps.new_chain(addr, 'd'), 'file', '/mapped.txt')
