# { "Depends": "py-genlayer:test" }
import genlayer as gl
from genlayer.vm.public_abi import Permissions

# 10 rotations => appealRounds = 9 => rounds span 0..18, which overruns the
# 18-entry validator table (indices 0..17). The bound is node-config-dependent,
# so the fee expression (not the SDK) rejects it with a clean `fee
# too_many_rounds` VMError instead of an internal abort.
_PARAMS = gl.chain.InternalMessageParams(
	leader_timeunits_allocation=5,
	validator_timeunits_allocation=5,
	execution_budget_per_round=1024,
	rotations=[4] * 10,
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
		# Metering aborts with `fee too_many_rounds`.
		gl.contract.get_at(gl.Address(b'\x30' * 20)).emit(
			use_balance=True, fee_params=_PARAMS
		).foo(1, 2)
