# { "Depends": "py-genlayer:test" }
import genlayer as gl

DEPLOYER = gl.Address('0x130e09c9aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa')


class Contract(gl.contract.Contract):
	def __init__(self):
		explicit = gl.contract.Salted(7, deployer=DEPLOYER, code_hash=b'\x42' * 32)
		print(gl.contract.deploy(code=b'not really a contract', salted=explicit))
		# The mock host has no EVM, so the predictGhostAddress call reverts the VM
		gl.contract.deploy(
			code=b'not really a contract',
			salted=gl.contract.Salted(7, deployer=DEPLOYER),
		)
