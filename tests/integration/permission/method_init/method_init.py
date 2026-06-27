# { "Depends": "py-genlayer:test" }
import genlayer as gl


class Contract(gl.contract.Contract):
	def __init__(self):
		print('init')
