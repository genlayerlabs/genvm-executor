# { "Depends": "py-genlayer:test" }
import genlayer as gl


class Contract(gl.contract.Contract):
	def __init__(self):
		salted = gl.contract.Salted(1)
		address = salted.resolve()
		assert salted.resolve() == address
		res = gl.contract.deploy(code=b'not really a contract', salted=salted)
		assert res == address
		print(res)
		assert gl.contract.deploy(code=b'not really a contract', salted=2) is None
