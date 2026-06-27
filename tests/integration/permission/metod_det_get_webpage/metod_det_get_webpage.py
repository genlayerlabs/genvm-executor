# { "Depends": "py-genlayer:test" }
import genlayer as gl


class Contract(gl.contract.Contract):
	def __init__(self):
		print('init')

	@gl.public.write
	def det_viol(self):
		gl.nondet.web.render(
			'https://test-server.genlayer.com/static/genvm/hello.html',
			mode='text',
		)
