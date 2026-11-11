# { "Depends": "py-genlayer:test" }
import genlayer as gl

DEPLOYER = gl.Address('0x130e09c9aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa')


class Contract(gl.contract.Contract):
	def __init__(self):
		explicit = gl.contract.Salted(7, deployer=DEPLOYER, code_hash=b'\x42' * 32)
		hardcoded = explicit.resolve_hardcoded()
		assert explicit.resolve() == hardcoded
		print(gl.contract.deploy(code=b'not really a contract', salted=explicit))
		# An unknown factory falls through to the mock host's reverting EVM handler
		gl.contract.deploy(
			code=b'not really a contract',
			salted=gl.contract.Salted(7, deployer=DEPLOYER),
		)
