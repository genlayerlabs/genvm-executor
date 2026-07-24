# { "Depends": "py-genlayer:test" }
import genlayer as gl
import genlayer._internal.on_chain.gl_call as gl_call
from genlayer.vm import _decode_sub_vm_result
from genlayer.vm import map_file, register_runner


class Contract(gl.contract.Contract):
	def __init__(self):
		# deterministic context registers a custom runner
		code = b'# { "Depends": "py-genlayer:test" }\nmarker\n'
		rid = register_runner(code)

		def leader():
			# burn a long loop first, then try to use the det-registered custom
			# runner from inside nondet. Passing custom_runners=[] grants no
			# custom runners, so this map MUST fail. The default Python SDK path
			# omits the field, which is defined as grant-all for compatibility.
			for _ in range(10**4):
				pass
			try:
				map_file(rid, 'file', '/mapped.txt')
				return 'nondet mapped det-registered runner (LEAK)'
			except Exception as e:
				return f'nondet map failed: {type(e).__name__}'

		import cloudpickle

		def validator_fn_mapped(_stage_data):
			return True

		print(
			gl_call.gl_call_generic(
				{
					'RunNondet': {
						'data_leader': cloudpickle.dumps(lambda _: leader()),
						'data_validator': cloudpickle.dumps(validator_fn_mapped),
						'custom_runners': [],
					}
				},
				_decode_sub_vm_result,
			)
		)
