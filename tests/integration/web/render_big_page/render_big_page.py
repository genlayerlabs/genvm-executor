# { "Depends": "py-genlayer:test" }
import genlayer as gl


class Contract(gl.contract.Contract):
	def __init__(self):
		def run():
			try:
				res = gl.nondet.web.render(
					'https://test-server.genlayer.com/big-page?kb=10240', mode='text'
				)
				return f'ok {len(res)}'
			except gl.nondet.NondetException as e:
				return f'error {e.causes} status={e.ctx.get("status")}'

		print(gl.eq_principle.strict_eq(run))
