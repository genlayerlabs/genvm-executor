# { "Depends": "py-genlayer:test" }
import genlayer as gl
from genlayer.vm.public_abi import Permissions

# A zero price cap is rejected structurally: the chain reverts
# `FeeValueMustBeNonZero` at reveal, so the SDK refuses it with Inval up front.
# Here `max_price_gen_per_time_unit` is zero (storage/receipt caps are checked
# the same way).
_PARAMS = gl.chain.InternalMessageParams(
	leader_timeunits_allocation=5,
	validator_timeunits_allocation=5,
	execution_budget_per_round=1024,
	rotations=[4, 4, 4, 4, 4],
	max_price_gen_per_time_unit=0,
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
		# use_balance with a zero price cap is rejected with Inval (errno 2).
		try:
			gl.contract.get_at(gl.Address(b'\x30' * 20)).emit(
				use_balance=True, fee_params=_PARAMS
			).foo(1, 2)
		except SystemError as e:
			print(e)
