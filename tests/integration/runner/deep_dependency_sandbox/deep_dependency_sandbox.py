# { "Depends": "py-genlayer:test" }
import json

import genlayer as gl
from genlayer.vm import register_runner


DEPTH = 64


def runner_bytes(action, body):
	return f'# {json.dumps(action)}\n{body}\n'.encode('utf-8')


class Contract(gl.contract.Contract):
	def __init__(self):
		prev = 'py-lib-cloudpickle:test'
		for idx in range(DEPTH):
			prev = register_runner(
				runner_bytes(
					{'Depends': prev},
					f'dependency_runner_{idx} = None',
				)
			)

		runner = {
			'Seq': [
				{'Depends': prev},
				{
					'With': {
						'action': {'MapFile': {'file': 'file', 'to': '/contract.py'}},
						'runner': 'contract',
					}
				},
				{'SetArgs': ['py', '-u', '-B', '/contract.py']},
				{'Depends': 'py-lib-cloudpickle:test'},
				{'Depends': 'py-lib-genlayer-std:test'},
				{'Depends': 'cpython:test'},
			]
		}
		top = register_runner(
			runner_bytes(
				runner,
				'\n'.join(
					[
						'print("deep custom runner ran")',
						"exec(open('/py/libs/_genlayer_bootloader.py').read())",
					]
				),
			)
		)

		res = gl.vm.spawn_sandbox(lambda: DEPTH, runner=top)
		print('sandbox ->', res)
