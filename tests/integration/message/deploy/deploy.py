# { "Depends": "py-genlayer:test" }
import genlayer as gl


class Contract(gl.contract.Contract):
	def __init__(self):
		gl.contract.deploy(code='not really a contract'.encode('utf-8'))
