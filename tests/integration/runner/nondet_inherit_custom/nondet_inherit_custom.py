# { "Depends": "py-genlayer:test" }
import genlayer as gl
from genlayer.vm import map_file, register_runner, run_nondet


class Contract(gl.contract.Contract):
	def __init__(self):
		# A `RunNondet` child inherits the parent's
		# entire custom-runner set by default (custom_runners=None). A runner
		# registered in the deterministic parent is therefore visible inside the
		# nondet block — the opposite of the pre-v2 blanket ban. The nondet child
		# performs its own load action for the inherited runner (charged to its own
		# limiter), so the same content is `charged` once per VM, never twice within
		# one VM.
		code = b'# { "Depends": "py-genlayer:test" }\nmarker\n'
		rid = register_runner(code)

		def leader():
			# would raise a malformed-runner error under the old (empty) nondet set
			map_file(rid, 'runner.json', '/inherited.json')
			return 'nondet mapped inherited custom'

		print(run_nondet(leader, lambda r: True))
