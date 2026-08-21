# { "Depends": "py-genlayer:test" }
import genlayer as gl
from genlayer.vm.public_abi import Permissions

# Proves the balance-funded floor scales with the GUEST cap, not the node's live
# genPerTimeUnit. With consensusTerm=5140 and executionTerm=1024*29=29696:
#   fee = max_price_gen_per_time_unit * consensusTerm + executionTerm
#       = 3 * 5140 + 29696 = 45116
# The jsonnet sets node.genPerTimeUnit=7; had the balance path used it the fee
# would be 7*5140 + 29696 = 65676. The golden's 45116 confirms the cap is used.
_PARAMS = gl.chain.InternalMessageParams(
	leader_time_units_allocation=5,
	validator_time_units_allocation=5,
	execution_budget_per_round=1024,
	rotations=[4, 4, 4, 4, 4],
	max_price_gen_per_time_unit=3,
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
