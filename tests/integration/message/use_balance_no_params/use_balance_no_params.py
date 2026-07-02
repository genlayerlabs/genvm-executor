# { "Depends": "py-genlayer:test" }
import genlayer as gl
from genlayer.vm.public_abi import Permissions


class Contract(gl.contract.Contract):
	def __init__(self):
		# Grant the permission first, otherwise the emission below would fail
		# with Forbidden (checked before the fee_params validation).
		gl.storage.Root.get().set_permission(
			Permissions.CAN_USE_BALANCE_FOR_MESSAGE_FEES, True
		)

	@gl.public.write
	def do_emit(self):
		# use_balance without fee_params is rejected with Inval (errno 2):
		# GenVM cannot meter the balance-funded fee without explicit params.
		try:
			gl.contract.get_at(gl.Address(b'\x30' * 20)).emit(use_balance=True).foo(1, 2)
		except SystemError as e:
			print(e)
