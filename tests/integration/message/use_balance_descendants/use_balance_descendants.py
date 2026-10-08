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
_TARGET = gl.contract.get_at(gl.Address(b'\x30' * 20))


class Contract(gl.contract.Contract):
	def __init__(self):
		gl.storage.Root.get().set_permission(
			Permissions.CAN_USE_BALANCE_FOR_MESSAGE_FEES, True
		)

	@gl.public.write
	def open(self):
		_TARGET.emit(
			use_balance=gl.contract.UseBalanceParams(_PARAMS, descendants=300_000)
		).foo()

	@gl.public.write
	def closed_zero(self):
		_TARGET.emit(use_balance=gl.contract.UseBalanceParams(_PARAMS, descendants=0)).foo()

	@gl.public.write
	def pinned_nested(self):
		self._emit_pinned(None)

	@gl.public.write
	def pinned_selector(self):
		self._emit_pinned(bytes.fromhex('a9059cbb'))

	def _emit_pinned(self, external_call_key):
		root = gl.message_allocation.InternalAllocation(
			gl.Address(b'\x11' * 20),
			bytes(32),
			_PARAMS,
			400_000,
			children=[
				gl.message_allocation.InternalAllocation(
					gl.Address(b'\x22' * 20),
					bytes(32),
					_PARAMS,
					270_000,
					on='decided',
				)
			],
		)
		external = gl.message_allocation.ExternalAllocation(
			gl.Address(b'\x33' * 20),
			external_call_key,
			gl.chain.ExternalMessageParams(20, 10),
			200,
		)
		_TARGET.emit(
			use_balance=gl.contract.UseBalanceParams(_PARAMS, descendants=[root, external]),
		).foo()

	@gl.public.write
	def invalid_tree(self):
		root = gl.message_allocation.InternalAllocation(
			gl.Address(b'\x11' * 20), None, _PARAMS, 60_000
		)
		duplicate = gl.message_allocation.InternalAllocation(
			gl.Address(b'\x11' * 20),
			gl.message_allocation.ANY_METHOD,
			_PARAMS,
			300_000,
			on='decided',
		)
		_TARGET.emit(
			use_balance=gl.contract.UseBalanceParams(_PARAMS, descendants=[root, duplicate]),
		).foo()

	@gl.public.write
	def external_budget(self):
		external = gl.message_allocation.ExternalAllocation(
			gl.Address(b'\x33' * 20),
			bytes.fromhex('a9059cbb'),
			gl.chain.ExternalMessageParams(20, 10),
			199,
		)
		_TARGET.emit(
			use_balance=gl.contract.UseBalanceParams(_PARAMS, descendants=[external]),
		).foo()

	@gl.public.write
	def duplicate_external_selector(self):
		selector = bytes.fromhex('a9059cbb')
		nodes: list[
			gl.message_allocation.InternalAllocation
			| gl.message_allocation.ExternalAllocation
		] = [
			gl.message_allocation.ExternalAllocation(
				gl.Address(b'\x33' * 20),
				key,
				gl.chain.ExternalMessageParams(20, 10),
				200,
			)
			for key in (selector, selector + bytes(28))
		]
		_TARGET.emit(
			use_balance=gl.contract.UseBalanceParams(_PARAMS, descendants=nodes)
		).foo()

	@gl.public.write
	def deploy_open(self):
		gl.contract.deploy(
			code=b'',
			use_balance=gl.contract.UseBalanceParams(_PARAMS, descendants=300_000),
		)

	@gl.public.write
	def nested_decided_underfunded(self):
		root = gl.message_allocation.InternalAllocation(
			gl.Address(b'\x11' * 20),
			bytes(32),
			_PARAMS,
			400_000,
			children=[
				gl.message_allocation.InternalAllocation(
					gl.Address(b'\x22' * 20),
					bytes(32),
					_PARAMS,
					267_379,
					on='decided',
				)
			],
		)
		_TARGET.emit(
			use_balance=gl.contract.UseBalanceParams(_PARAMS, descendants=[root]),
		).foo()
