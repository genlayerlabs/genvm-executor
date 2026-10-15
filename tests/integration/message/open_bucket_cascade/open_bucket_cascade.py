# { "Depends": "py-genlayer:test" }
import genlayer as gl
from genlayer.vm.public_abi import Permissions

_PARAMS = gl.chain.InternalMessageParams(
	leader_time_units_allocation=5,
	validator_time_units_allocation=5,
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
	def emit(self):
		target = gl.contract.get_at(gl.Address(b'\x30' * 20))
		# Both sender-pool messages are floored, then share half of the spare
		# budget; the balance-funded one keeps its metered fee
		target.emit().foo(1)
		target.emit(use_balance=gl.contract.UseBalanceParams(_PARAMS)).foo(2)
		target.emit(on='decided').foo(3)
