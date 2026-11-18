# { "Depends": "py-genlayer:test" }

import genlayer as gl
import genlayer._internal.on_chain.gl_call as gl_call

CAP = 256 << 10


def call(size: int):
	gl_call.gl_call_generic(
		{
			'ExternalCall': {
				'address': gl.Address(b'\x03' * 20),
				'calldata': b'\x01' * size,
			}
		},
		lambda x: x,
	).get()


class Contract(gl.contract.Contract):
	def __init__(self):
		pass

	@gl.public.view
	def at_cap(self):
		call(CAP)

	@gl.public.view
	def over_cap(self):
		try:
			call(CAP + 1)
		except Exception as e:
			print('refused:', type(e).__name__, e)
