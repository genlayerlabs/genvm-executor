# { "Depends": "py-genlayer:test" }
import genlayer as gl

# A well formed `name:<canonical gvm32>` pair that is simply not installed. It
# gets past the format and gvm32 checks and only fails the registry lookup, so
# it is the one shape that used to abort the transaction internally instead of
# producing a canonical result.
_ABSENT = 'py-genlayer:8b8kjyda2ycxyq4ea6g4yfpnydxhd52gqba5rb8dw7krkh5mn9p0'
# Same shape, but the trailing character carries non-zero padding bits, so it
# is rejected earlier, by the gvm32 decoder.
_MALFORMED = 'py-genlayer:9b8kjyda2ycxyq4ea6g4yfpnydxhd52gqba5rb8dw7krkh5mn9p1'


class Contract(gl.contract.Contract):
	def __init__(self):
		pass

	@gl.public.write
	def spawn_absent(self):
		gl.vm.spawn_sandbox(lambda: 1, runner=_ABSENT)

	@gl.public.write
	def map_absent(self):
		gl.vm.map_file(_ABSENT, 'runner.json', '/tmp/probe')

	@gl.public.write
	def spawn_malformed(self):
		gl.vm.spawn_sandbox(lambda: 1, runner=_MALFORMED)
