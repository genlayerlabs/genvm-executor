# { "Depends": "py-genlayer:test" }
import genlayer as gl
from genlayer.types import Address


class Contract(gl.contract.Contract):
	def __init__(self):
		pass

	@gl.public.write
	def main(self, addr: Address):
		# Opted in: B's VM error arrives as this call's result, so it is an
		# ordinary Python exception here.
		try:
			gl.contract.get_at(addr).view(catch_vm_error=True).boom()
			print('A: no error')
		except Exception as e:
			print(f'A caught {e}')

		# Not opted in: the same error ends A too, so nothing below runs.
		gl.contract.get_at(addr).view().boom()
		print('A: UNREACHABLE')
