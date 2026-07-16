# { "Depends": "py-genlayer:test" }
import genlayer as gl


class Contract(gl.contract.Contract):
	def __init__(self):
		pass

	@gl.public.view
	def fib(self, a):
		print(gl.message.stack)
		if a <= 1:
			return a
		else:
			zelf = gl.contract.get_at(gl.Address(self.address))
			return zelf.view().fib(a - 1) + zelf.view().fib(a - 2)
