# { "Depends": "py-genlayer:test" }
import genlayer as gl
from genlayer.vm.public_abi import Permissions

# Proves the balance-funded floor scales with the GUEST cap and grosses up only
# the time-unit pool. With timeUnitPool=3*5140, overlay=floor(15420*1500/8500),
# and executionTerm=1024*29:
#   primary = 15420 + 2721 + 29696 = 47837
# The per-message fee is 47837 for both decided and finalized emissions
# The jsonnet sets node.genPerTimeUnit=7; had the balance path used it the fee
# would be 7*5140 + floor(35980*1500/8500) + 29696 = 72025
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
			on='decided', use_balance=True, fee_params=_PARAMS
		).foo(1, 2)
