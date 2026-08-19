# { "Depends": "py-genlayer:test" }
import genlayer as gl
from genlayer.vm import map_file, register_runner, spawn_sandbox


class Contract(gl.contract.Contract):
	def __init__(self):
		# No flow-back: a runner registered inside a sandbox subtree
		# lives only in that child's loaded set and dies with it. The parent never
		# loaded it, so resolving its id in the parent afterwards is a deterministic
		# malformed-runner error — registrations do not flow back up.
		code = b'# { "Depends": "py-genlayer:test" }\nx\n'

		def inner():
			# runs in the sandbox child; registers into the child's own set
			return register_runner(code)

		res = spawn_sandbox(inner)
		rid = gl.vm.unpack_result(res)
		print('sandbox registered', rid)

		# The parent never loaded `rid`; resolving it must fail deterministically.
		map_file(rid, 'runner.json', '/leaked.json')
		print('LEAK: parent resolved a child-scoped registration')
