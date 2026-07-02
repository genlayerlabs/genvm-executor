# { "Depends": "py-genlayer:test" }
import genlayer as gl
from genlayer.vm.public_abi import Permissions

# leader/validator timeunits (5) are below the node's minTimeUnitsPerPhase floor
# (30, set in the jsonnet), so metering rejects the emission.
_PARAMS = gl.chain.InternalMessageParams(
	leader_timeunits_allocation=5,
	validator_timeunits_allocation=5,
	execution_budget_per_round=1024,
	rotations=[4, 4, 4, 4, 4],
	max_price_gen_per_time_unit=2,
	storage_fee_max_gas_price=20,
	receipt_fee_max_gas_price=20,
)


class Contract(gl.contract.Contract):
	def __init__(self):
		gl.storage.Root.get().set_permission(
			Permissions.CAN_USE_BALANCE_FOR_MESSAGE_FEES, True
		)

	@gl.public.write
	def do_emit(self):
		# The min-timeunits floor is enforced on the balance-funded path too, so
		# emission aborts with the `fee below_minimum` VMError.
		gl.contract.get_at(gl.Address(b'\x30' * 20)).emit(
			use_balance=True, fee_params=_PARAMS
		).foo(1, 2)
