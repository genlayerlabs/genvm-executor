# { "Depends": "py-genlayer:test" }
import genlayer as gl
from genlayer.types import Address, u256


@gl.evm.contract_interface
class ExternalTarget:
	class View:
		pass

	class Write:
		def call(self, value: u256, /) -> None: ...


class Contract(gl.contract.Contract):
	def __init__(self):
		ExternalTarget(Address(b'\x30' * 20)).emit().call(1)
