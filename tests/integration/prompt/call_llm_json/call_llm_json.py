# { "Depends": "py-genlayer:test" }
import typing

import genlayer as gl


class Contract(gl.contract.Contract):
	def __init__(self):
		def run():
			res = gl.nondet.exec_prompt(
				'respond with json object containing single key "result" and associated value being a random integer from 0 to 100 (inclusive), it must be number, not wrapped in quotes',
				response_format='json',
			)
			if not isinstance(res, dict):
				raise TypeError(f'invalid result {res!r}')
			result = res.get('result')
			if (
				not isinstance(result, int)
				or isinstance(result, bool)
				or not 0 <= result <= 100
			):
				raise TypeError(f'invalid result {result!r}')
			res['result'] = 42
			return typing.cast(gl.calldata.Decoded, res)

		print(gl.eq_principle.strict_eq(run))
