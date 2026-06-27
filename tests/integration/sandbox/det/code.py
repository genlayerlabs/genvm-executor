# { "Depends": "py-genlayer:test" }
import json

import genlayer as gl


class Contract(gl.contract.Contract):
	@gl.public.write
	def main(self, ev: str):
		glb = globals()
		print(f'{gl.vm.spawn_sandbox(lambda: eval(ev, glb))}')
		print(json.loads.__name__)
