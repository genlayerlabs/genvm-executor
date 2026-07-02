# { "Depends": "py-genlayer:test" }
import genlayer as gl
from genlayer.vm.public_abi import Permissions

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
		# The contract balance (100) is below the metered message fee, so the
		# balance-funded emission fails with Inbalance (errno 7), catchable by
		# the guest just like a value overspend.
		try:
			gl.contract.get_at(gl.Address(b'\x30' * 20)).emit(
				use_balance=True, fee_params=_PARAMS
			).foo(1, 2)
		except SystemError as e:
			print(e)
