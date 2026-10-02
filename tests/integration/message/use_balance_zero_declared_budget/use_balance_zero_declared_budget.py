# { "Depends": "py-genlayer:test" }
import genlayer as gl
from genlayer.vm.public_abi import Permissions

# Zero time units opt out of phase timeouts and a zero execution budget skips
# the budget floor, so every fee term is zero: the child would carry no budget
_PARAMS = gl.chain.InternalMessageParams(
	leader_time_units_allocation=0,
	validator_time_units_allocation=0,
	execution_budget_per_round=0,
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
		gl.contract.get_at(gl.Address(b'\x30' * 20)).emit(
			use_balance=True, fee_params=_PARAMS
		).foo(1, 2)
