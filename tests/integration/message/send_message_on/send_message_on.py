# { "Depends": "py-genlayer:test" }
import typing

import genlayer as gl


class Contract(gl.contract.Contract):
	@gl.public.write
	def main(self, on: str):
		if typing.TYPE_CHECKING:
			assert on in ('finalized', 'decided'), f'Invalid on value: {on}'
		try:
			gl.contract.get_at(gl.Address(b'\x30' * 20)).emit(on=on).foo(1, 2)
		except SystemError as e:
			print(e)
