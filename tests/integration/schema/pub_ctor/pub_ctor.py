# { "Depends": "py-genlayer:test" }
import genlayer as gl


class Contract(gl.contract.Contract):
	@gl.public.write
	def __init__(self):
		print('hello world')
