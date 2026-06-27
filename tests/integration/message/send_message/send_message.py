# { "Depends": "py-genlayer:test" }
import genlayer as gl


class Contract(gl.contract.Contract):
	def __init__(self):
		gl.contract.get_at(gl.Address(b'\x30' * 20)).emit().foo(1, 2)
