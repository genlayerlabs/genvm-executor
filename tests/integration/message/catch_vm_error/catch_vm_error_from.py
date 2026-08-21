# { "Depends": "py-genlayer:test" }
import genlayer as gl
from genlayer.types import Address


class Contract(gl.contract.Contract):
	def __init__(self):
		pass

	@gl.public.write
	def main(self, addr: Address):
		# Opted in: B's VM error arrives as this call's result.
		print('A got', gl.contract.get_at(addr).view(catch_vm_error=True).boom())

		# Not opted in: the same error ends A too, so nothing below runs.
		gl.contract.get_at(addr).view().boom()
		print('A: UNREACHABLE')
