# { "Depends": "py-genlayer:test" }
from genlayer import *


class Contract(gl.Contract):
	def __init__(self):
		pass

	@gl.public.write
	def foo(self, a, b):
		print('contract to.foo')
		gl.advanced.user_error_immediate(f"nah, I won't execute {a + b}")
