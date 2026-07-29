# { "Depends": "py-genlayer:9b8kjyda2ycxyq4ea6g4yfpnydxhd52gqba5rb8dw7krkh5mn9p0" }
import genlayer as gl


class Contract(gl.contract.Contract):
	def __init__(self):
		pass

	@gl.public.view
	def answer(self) -> int:
		print('routed_call to.answer')
		return 42
