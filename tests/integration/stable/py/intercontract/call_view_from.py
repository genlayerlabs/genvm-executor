# { "Depends": "py-genlayer:test" }
from genlayer import *


class Contract(gl.Contract):
	def __init__(self):
		pass

	@gl.public.write
	def main(self, addr: Address):
		print('contract from.main')
		print(gl.get_contract_at(addr).view().foo(1, 2))
