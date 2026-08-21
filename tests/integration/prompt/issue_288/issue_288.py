# { "Depends": "py-genlayer:test" }
import sys

import genlayer as gl


class Contract(gl.contract.Contract):
	def __init__(self):
		def run():
			v = gl.nondet.exec_prompt(
				'Respond with a json object with a single key "random" and value between 0 and 1, like 0.5',
				response_format='json',
			)
			print(v, file=sys.stderr)
			print(type(v).__name__)
			if not isinstance(v, dict):
				raise TypeError(f'invalid result {v!r}')
			r = v.get('random')
			if not isinstance(r, int | float) or isinstance(r, bool):
				raise TypeError(f'invalid random value {r!r}')
			print(type(r).__name__)
			print(r >= 0 and r <= 1)

		gl.eq_principle.strict_eq(run)
