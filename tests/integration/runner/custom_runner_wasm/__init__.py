import genlayer as gl

# A wasm file is a runner archive on its own, so the bytes shipped alongside
# this contract can be registered as-is and then run as a sandbox child.
WASM_IN_RUNNER = 'fibonacci.wasm'


class Contract(gl.contract.Contract):
	def __init__(self):
		gl.vm.map_file(gl.vm.RunnerIDOps.CONTRACT, WASM_IN_RUNNER, '/fibonacci.wasm')
		with open('/fibonacci.wasm', 'rb') as f:
			code = f.read()

		rid = gl.vm.register_runner(code)
		print('registered', rid.split(':')[0])

		for n in (20, 30):
			# No permissions: the child only computes and answers. Both
			# directions of the boundary are calldata, so neither side has to
			# agree on a bespoke byte layout.
			answer = gl.vm.spawn_runner(rid, gl.calldata.encode(n))
			answer = gl.vm.unpack_result(answer)
			# we have two layers of calldata encoding here due to a Rust SDK type
			assert isinstance(answer, bytes), f'expected bytes, got {type(answer).__name__}'
			print(f'fib({n}) =', gl.calldata.decode(answer))
