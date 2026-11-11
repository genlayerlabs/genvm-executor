import sys
import types
import typing
from functools import partial

import genlayer.evm as genvm_eth
import pytest
from genlayer import calldata
from genlayer.evm.calldata import MethodEncoder
from genlayer.types import Address, u256


def generate_test(
	name: str, params: tuple[type], ret: type, *, dump_to: list
) -> typing.Any:
	encoder = MethodEncoder(name, params, ret)

	def result_fn(self, *args):
		dump_to.append(self._proxy_parent.address)
		dump_to.append(encoder.encode_call(args))

	return result_fn


def test_evm_transfer_remains_an_external_message(monkeypatch):
	monkeypatch.setitem(sys.modules, '_genlayer_wasi', types.ModuleType('_genlayer_wasi'))
	from genlayer._internal.on_chain import eth

	calls = []

	def emit(data):
		calls.append(calldata.decode(data))
		return 2**32 - 1

	monkeypatch.setattr(eth.gl_call, '_imp_raw', emit, raising=False)

	@eth.evm_contract_interface
	class EVMContract:
		class View:
			pass

		class Write:
			pass

	address = Address(b'\x01' * 20)
	assert EVMContract(address).emit_transfer(7) is None
	assert calls == [
		{'EmitExternalMessage': {'address': address, 'calldata': b'', 'value': 7}}
	]


@pytest.mark.parametrize(
	'address, value, error',
	[
		(Address.ZERO, 7, 'zero address'),
		(Address(b'\x01' * 20), 0, 'greater than 0'),
		(Address(b'\x01' * 20), -1, 'greater than 0'),
	],
)
def test_evm_transfer_validates_before_encoding(monkeypatch, address, value, error):
	monkeypatch.setitem(sys.modules, '_genlayer_wasi', types.ModuleType('_genlayer_wasi'))
	from genlayer._internal.on_chain import eth

	def unexpected(*args, **kwargs):
		pytest.fail('invalid transfer must fail before encoding or calling the VM')

	monkeypatch.setattr(calldata, 'encode', unexpected)
	monkeypatch.setattr(eth.gl_call, '_imp_raw', unexpected, raising=False)

	@eth.evm_contract_interface
	class EVMContract:
		class View:
			pass

		class Write:
			pass

	with pytest.raises(ValueError, match=error):
		EVMContract(address).emit_transfer(value)


def test_named_evm_send_still_allows_zero_value(monkeypatch):
	monkeypatch.setitem(sys.modules, '_genlayer_wasi', types.ModuleType('_genlayer_wasi'))
	from genlayer._internal.on_chain import eth

	calls = []

	def emit(data):
		calls.append(calldata.decode(data))
		return 2**32 - 1

	monkeypatch.setattr(eth.gl_call, '_imp_raw', emit, raising=False)

	@eth.evm_contract_interface
	class EVMContract:
		class View:
			pass

		class Write:
			def run(self) -> None: ...

	address = Address(b'\x01' * 20)
	EVMContract(address).emit().run()
	assert calls == [
		{
			'EmitExternalMessage': {
				'address': address,
				'calldata': MethodEncoder('run', (), type(None)).encode_call(()),
				'value': 0,
			}
		}
	]


def test_view_send():
	tst = []
	transfers = []
	generator = partial(generate_test, dump_to=tst)

	@genvm_eth.contract_generator(
		generator,
		generator,
		lambda x: 0,
		lambda p, d: transfers.append((p.address, d)),
	)
	class MyContract:
		class View:
			def foo(self, param: str, /): ...

		class Write:
			def bar(self, param: str, /): ...

	addr = Address(b'\x00' * 20)
	contr = MyContract(addr)
	assert tst == []
	contr.view().foo('123')
	assert tst == [
		addr,
		b'\xf3\x1aii\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00 \x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x03123\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00',
	]

	tst.clear()
	contr.emit().bar('abc')
	assert tst == [
		addr,
		b'\xd4s\xa8\xed\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00 \x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x03abc\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00',
	]

	contr.emit_transfer(typing.cast(u256, 7))
	assert transfers == [(addr, {'value': 7})]
	with pytest.raises(TypeError):
		contr.emit_transfer()  # type: ignore[call-arg]
	with pytest.raises(TypeError):
		contr.emit_transfer(7, on='finalized')  # type: ignore[call-arg]
