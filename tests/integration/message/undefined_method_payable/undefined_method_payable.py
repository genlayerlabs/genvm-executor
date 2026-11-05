# { "Depends": "py-genlayer:test" }

import typing

import genlayer as gl


class Contract(gl.contract.Contract):
	def __handle_undefined_method_payable__(
		self, method_name: str, args: list[typing.Any], kwargs: dict[str, typing.Any]
	):
		print(
			{
				'me': '__handle_undefined_method_payable__',
				'method_name': method_name,
				'args': args,
				'kwargs': kwargs,
				'value': gl.message.value,
			}
		)

	def __receive__(self):
		print({'me': '__receive__', 'value': gl.message.value})
