import genlayer as gl

from . import lib


class Contract(gl.contract.Contract):
	def __init__(self):
		lib.foo()
