# { "Depends": "py-genlayer:test" }
import genlayer as gl
from genlayer.vm.public_abi import Permissions

_PARAMS = gl.chain.InternalMessageParams(
	leader_timeunits_allocation=5,
	validator_timeunits_allocation=5,
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
		# Grant the permission in this earlier deploy transaction.
		gl.storage.Root.get().set_permission(
			Permissions.CAN_USE_BALANCE_FOR_MESSAGE_FEES, True
		)

	@gl.public.write
	def do_emit(self):
		# A Sandbox sub-VM inherits CAN_USE_BALANCE_FOR_MESSAGE_FEES from the
		# top-level contract, so a use_balance emission from inside the sandbox
		# (spawned with allow_send_messages) succeeds.
		def sandbox_fn():
			try:
				gl.contract.get_at(gl.Address(b'\x30' * 20)).emit(
					use_balance=True, fee_params=_PARAMS
				).foo(1, 2)
				print('sandbox: emitted')
			except SystemError as e:
				print(f'sandbox: {e}')

		gl.vm.spawn_sandbox(sandbox_fn, allow_send_messages=True)
