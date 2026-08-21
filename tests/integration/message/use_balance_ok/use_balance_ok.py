# { "Depends": "py-genlayer:test" }
import genlayer as gl
from genlayer.vm.public_abi import Permissions

_PARAMS = gl.chain.InternalMessageParams(
	leader_time_units_allocation=5,
	validator_time_units_allocation=5,
	execution_budget_per_round=1024,
	rotations=[4, 4, 4, 4, 4],
	# Small GEN cap keeps the metered fee tractable: it is the multiplier for the
	# consensus term (fee = cap * consensusTerm + executionTerm).
	max_price_gen_per_time_unit=2,
	storage_fee_max_gas_price=20,
	receipt_fee_max_gas_price=20,
)


class Contract(gl.contract.Contract):
	def __init__(self):
		# Permissions are read pre-execution, so the bit must be granted in this
		# (earlier) deploy transaction; the emission happens in the next one.
		gl.storage.Root.get().set_permission(
			Permissions.CAN_USE_BALANCE_FOR_MESSAGE_FEES, True
		)

	@gl.public.write
	def do_emit(self):
		# The permission is now set (prior step), so the balance-funded emission
		# succeeds. The metered fee is charged against this contract's balance
		# and the emitted message carries `use_balance=true` with an empty subtree.
		gl.contract.get_at(gl.Address(b'\x30' * 20)).emit(
			use_balance=True, fee_params=_PARAMS
		).foo(1, 2)
