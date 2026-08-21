# { "Depends": "py-genlayer:test" }
import genlayer as gl
from genlayer.vm.public_abi import Permissions

_PARAMS = gl.chain.InternalMessageParams(
	leader_time_units_allocation=5,
	validator_time_units_allocation=5,
	execution_budget_per_round=1024,
	rotations=[4, 4, 4, 4, 4],
	# Small GEN cap keeps the metered fee tractable (fee = cap * consensusTerm +
	# executionTerm).
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
	def emit_normal(self):
		# The tx carries an empty message-fee allocation tree, so the ordinary
		# (allocation-matched) path finds no matching node and aborts with the
		# `fee no_matching_allocation` VMError.
		gl.contract.get_at(gl.Address(b'\x30' * 20)).emit().foo(1, 2)

	@gl.public.write
	def emit_balance(self):
		# use_balance bypasses allocation matching entirely, so the same
		# emission succeeds despite the empty tree.
		gl.contract.get_at(gl.Address(b'\x30' * 20)).emit(
			use_balance=True, fee_params=_PARAMS
		).foo(1, 2)
