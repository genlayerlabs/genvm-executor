# { "Depends": "py-genlayer:test" }
import genlayer as gl


class Contract(gl.contract.Contract):
	def __init__(self):
		for value in range(3):
			assert gl.eq_principle.strict_eq(lambda: value) == value
		print('validated 3 nondeterministic blocks')
