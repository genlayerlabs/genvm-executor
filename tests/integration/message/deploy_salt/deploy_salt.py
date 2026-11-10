# { "Depends": "py-genlayer:test" }
import genlayer as gl


class Contract(gl.contract.Contract):
	def __init__(self):
		res = gl.contract.deploy(
			code='not really a contract'.encode('utf-8'), salted=gl.contract.Salted(1)
		)
		print(res)
