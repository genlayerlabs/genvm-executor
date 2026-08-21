import typing
from functools import partial

import genlayer.evm as genvm_eth
import pytest
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
