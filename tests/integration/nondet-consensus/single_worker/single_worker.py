# { "Depends": "py-genlayer:test" }
import genlayer as gl


class Contract(gl.contract.Contract):
	def __init__(self):
		for i in range(3):

			def nondet():
				print('nondet', i)
				return i

			print('before', i)
			assert gl.eq_principle.strict_eq(nondet) == i
			print('after', i)
		print('validated 3 nondeterministic blocks')
