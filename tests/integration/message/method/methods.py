# { "Depends": "py-genlayer:test" }
import genlayer as gl


class Contract(gl.contract.Contract):
	def __init__(self):
		print('init')

	@gl.public.write
	def pub(self):
		eval("print('init from pub!')")

	@gl.public.write
	def rback(self):
		gl.vm.UserError.immediate("nah, I won't execute")

	@gl.public.write
	def retn(self):
		return {'x': 10}

	@gl.public.view
	def retn_view(self):
		return {'x': 10}
