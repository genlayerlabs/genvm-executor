# { "Depends": "py-genlayer:test" }
import genlayer as gl
from genlayer.vm.public_abi import Permissions

# Params of huge magnitude are rejected structurally: unbounded, the metered
# floor (~2^500 here) would exceed U256 inside the fee evaluator and abort the
# executor internally. Prices/budgets are bounded to < 2^96, counts (time
# units, rotations entries) to < 2^32.
_PARAMS = gl.chain.InternalMessageParams(
	leader_time_units_allocation=2**250,
	validator_time_units_allocation=0,
	execution_budget_per_round=0,
	rotations=[0],
	max_price_gen_per_time_unit=2**250,
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
		# use_balance with out-of-bounds magnitudes is rejected with Inval
		# (errno 2), not an internal abort.
		try:
			gl.contract.get_at(gl.Address(b'\x30' * 20)).emit(
				use_balance=True, fee_params=_PARAMS
			).foo(1, 2)
		except SystemError as e:
			print(e)
