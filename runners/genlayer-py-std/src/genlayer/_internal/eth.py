__all__ = ('evm_contract_interface',)

import typing

import _genlayer_wasi as wasi

import genlayer._internal.on_chain.gl_call as gl_call
from genlayer.evm.calldata import MethodEncoder, decode
from genlayer.evm.generate import contract_generator

from . import _lazy_api


def _generate_view(name: str, params: tuple[type], ret: type) -> typing.Any:
	encoder = MethodEncoder(name, params, ret)

	def result_fn(self, *args):
		calldata = encoder.encode_call(args)
		return gl_call.gl_call_generic(
			{
				'ExternalCall': {
					'address': self._proxy_parent.address,
					'calldata': calldata,
				}
			},
			lambda x: decode(ret, x),
		)

	return _lazy_api(result_fn)


def _generate_send(name: str, params: tuple[type], ret: type) -> typing.Any:
	encoder = MethodEncoder(name, params, ret)

	def result_fn(self, *args):
		calldata = encoder.encode_call(args)
		if len(self._proxy_args) != 1:
			raise TypeError(f'expected exactly 1 proxy arg, got {len(self._proxy_args)}')
		if len(self._proxy_kwargs) != 0:
			raise TypeError(
				f'expected no proxy kwargs, got {sorted(self._proxy_kwargs.keys())}'
			)
		gl_call.gl_call_generic(
			{
				'EmitExternalMessage': {
					'address': self._proxy_parent.address,
					'calldata': calldata,
					'value': self._proxy_kwargs.get('value', 0),
				}
			},
			lambda _x: None,
		).get()

	return result_fn


evm_contract_interface = contract_generator(
	_generate_view,
	_generate_send,
	lambda p: wasi.get_balance(p.address.as_bytes),
	lambda p, d: gl_call.gl_call_generic(
		{
			'EmitExternalMessage': {
				'address': p.address,
				'calldata': b'',
				'value': d.get('value', 0),
			}
		},
		lambda _x: None,
	).get(),
)

evm_contract_interface.__doc__ = """
Decorator that is used to declare eth contract interface

.. code:: python

	@gl.evm.contract_interface
	class Ghost:
		class View:
			pass

		class Write:
			def test(self, x: u256, /) -> None: ...
"""
