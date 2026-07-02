# { "Depends": "py-genlayer:test" }
import genlayer as gl

_PARAMS = gl.chain.InternalMessageParams(
	leader_timeunits_allocation=5,
	validator_timeunits_allocation=5,
	execution_budget_per_round=1024,
	rotations=[4, 4, 4, 4, 4],
	max_price_gen_per_time_unit=2**200,
	storage_fee_max_gas_price=20,
	receipt_fee_max_gas_price=20,
)


class Contract(gl.contract.Contract):
	def __init__(self):
		# The CAN_USE_BALANCE_FOR_MESSAGE_FEES bit is unset (no earlier step
		# granted it), so a balance-funded emission is rejected pre-execution
		# with Forbidden (errno 6). Caught here so the result stays `Return`.
		try:
			gl.contract.get_at(gl.Address(b'\x30' * 20)).emit(
				use_balance=True, fee_params=_PARAMS
			).foo(1, 2)
		except SystemError as e:
			print(e)
