# { "Depends": "py-genlayer:test" }
import genlayer as gl


class Contract(gl.contract.Contract):
	@gl.public.write.payable
	def __receive__(self):
		pass
