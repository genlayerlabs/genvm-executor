# { "Depends": "py-genlayer:test" }
import genlayer as gl


class Contract(gl.contract.Contract):
	def __init__(self):
		# Emits a single internal message. Its matched allocation funds child
		# timeunits (leader 5, validator 10) below the node's minTimeUnitsPerPhase
		# floor (30), so emission is rejected with `fee below_minimum` and the
		# message is never issued.
		gl.contract.get_at(gl.Address(b'\x30' * 20)).emit().foo(1, 2)
