import types
import typing

import genlayer as gl
import genlayer.calldata as calldata
import genlayer.contract as contract
import pytest
from genlayer.chain import InternalMessageParams
from genlayer.message_allocation import (
	ANY_METHOD,
	ExternalAllocation,
	InternalAllocation,
	UseBalanceParams,
	_resolve_call_key,
)
from genlayer.types import Address, Keccak256


def params() -> InternalMessageParams:
	return InternalMessageParams(5, 10, 100, [0], 1, 10**18, 10**18)


def test_message_allocation_module_is_available_from_package():
	assert gl.message_allocation.InternalAllocation is InternalAllocation
	assert 'message_allocation' in gl.__all__


def test_account_funding_type_hints_resolve():
	for account in (gl.chain.IAccount, gl.chain.Account):
		assert (
			typing.get_type_hints(account.emit_transfer)['use_balance']
			== UseBalanceParams | None
		)


def test_call_key_resolution_matches_rust_sdk():
	assert _resolve_call_key(None) == ANY_METHOD
	assert _resolve_call_key(bytes(32)) == bytes(32)
	assert _resolve_call_key('') == bytes(32)
	assert _resolve_call_key(b'\x07' * 32) == b'\x07' * 32
	assert _resolve_call_key('run') == b'run' + bytes(29)

	name = 'x' * 32
	expected = bytearray(Keccak256(name.encode()).digest())
	expected[-1] |= 1
	assert _resolve_call_key(name) == bytes(expected)
	assert ANY_METHOD == Keccak256(b'').digest()


@pytest.mark.parametrize('invalid', [b'', b'1' * 4, b'1' * 31, b'1' * 33, 7, False])
def test_call_key_resolution_rejects_invalid_values(invalid):
	with pytest.raises((TypeError, ValueError)):
		_resolve_call_key(invalid)


@pytest.mark.parametrize('name', ['a' * 31, 'a' * 32, 'é' * 15, 'é' * 16, 'é' * 40])
def test_internal_call_key_uses_utf8_byte_length(name):
	raw = name.encode()
	if len(raw) < 32:
		expected = raw + bytes(32 - len(raw))
	else:
		expected = bytearray(Keccak256(raw).digest())
		expected[-1] |= 1
		expected = bytes(expected)
	assert _resolve_call_key(name) == expected


@pytest.mark.parametrize('kind', ['call', 'transfer', 'deploy'])
@pytest.mark.parametrize('external', [False, True])
@pytest.mark.parametrize(
	'key, expected',
	[
		(None, ANY_METHOD),
		(ANY_METHOD, ANY_METHOD),
		(bytes(32), bytes(32)),
		(bytes(range(32)), bytes(range(32))),
	],
)
def test_allocation_keys_survive_every_emission_path(
	monkeypatch, kind, external, key, expected
):
	calls = []
	monkeypatch.setattr(
		contract, 'wasi', types.SimpleNamespace(gl_call=calls.append), raising=False
	)
	allocation = (
		ExternalAllocation(Address.ZERO, key, gl.chain.ExternalMessageParams(1, 1), 1)
		if external
		else InternalAllocation(Address.ZERO, key, params(), 100)
	)
	funding = UseBalanceParams(params(), [allocation])
	proxy = contract.get_at(Address.ZERO)
	if kind == 'call':
		proxy.emit(funding).run()
	elif kind == 'transfer':
		proxy.emit_transfer(1, funding)
	else:
		contract.deploy(funding, code=b'code')
	message = next(iter(calldata.decode(calls.pop()).values()))
	assert message['descendants'][0]['call_key'] == expected
	assert message['descendants'][0]['parent_index'] == 2**256 - 1


@pytest.mark.parametrize('selector', [bytes(4), bytes.fromhex('a9059cbb'), b'\xff' * 4])
@pytest.mark.parametrize('kind', ['call', 'transfer', 'deploy'])
def test_external_allocation_pads_selector_on_the_right(monkeypatch, selector, kind):
	calls = []
	monkeypatch.setattr(
		contract, 'wasi', types.SimpleNamespace(gl_call=calls.append), raising=False
	)
	node = ExternalAllocation(
		Address.ZERO, selector, gl.chain.ExternalMessageParams(1, 1), 1
	)
	funding = UseBalanceParams(params(), [node])
	proxy = contract.get_at(Address.ZERO)
	if kind == 'call':
		proxy.emit(funding).run()
	elif kind == 'transfer':
		proxy.emit_transfer(1, funding)
	else:
		contract.deploy(funding, code=b'code')
	message = next(iter(calldata.decode(calls.pop()).values()))
	assert message['descendants'][0]['call_key'] == selector + bytes(28)


@pytest.mark.parametrize(
	'invalid',
	[
		b'',
		b'1',
		b'123',
		b'12345',
		b'1' * 31,
		b'1' * 33,
		'',
		'transfer',
		'transfer(address,uint256)',
		7,
		False,
		bytearray(4),
		memoryview(bytes(4)),
	],
)
def test_external_allocation_rejects_invalid_keys_before_emission(monkeypatch, invalid):
	calls = []
	monkeypatch.setattr(
		contract, 'wasi', types.SimpleNamespace(gl_call=calls.append), raising=False
	)
	node = ExternalAllocation(
		Address.ZERO, invalid, gl.chain.ExternalMessageParams(1, 1), 1
	)
	with pytest.raises((TypeError, ValueError)):
		contract.get_at(Address.ZERO).emit(UseBalanceParams(params(), [node])).run()
	assert calls == []


def test_allocations_encode_call_keys_and_shapes():
	internal = InternalAllocation(
		Address.ZERO,
		'run',
		params(),
		300,
		on='decided',
		children=[],
	)
	external = ExternalAllocation(
		Address.ZERO,
		ANY_METHOD,
		gl.chain.ExternalMessageParams(200_000, 10**9),
		200_000_000_000_000,
	)

	internal_value = internal.__to_calldata__()
	assert internal_value['call_key'] == b'run' + bytes(29)
	assert internal_value['on'] == 'decided'
	assert 'children' not in internal_value

	external_value = external.__to_calldata__()
	assert external_value['call_key'] == ANY_METHOD
	assert 'on' not in external_value
	assert 'children' not in external_value


class Zero(int):
	pass


@pytest.mark.parametrize('descendants', [None, 0, Zero(0), [], ()])
def test_closed_descendants_are_omitted_from_emit(monkeypatch, descendants):
	calls = []
	monkeypatch.setattr(
		contract, 'wasi', types.SimpleNamespace(gl_call=calls.append), raising=False
	)

	contract.get_at(Address.ZERO).emit(
		UseBalanceParams(fee_params=params(), descendants=descendants),
	).run()

	message = calldata.decode(calls.pop())['EmitInternalMessage']
	assert 'descendants' not in message


def test_open_and_pinned_descendants_are_forwarded(monkeypatch):
	calls = []
	monkeypatch.setattr(
		contract, 'wasi', types.SimpleNamespace(gl_call=calls.append), raising=False
	)
	proxy = contract.get_at(Address.ZERO)

	proxy.emit(use_balance=UseBalanceParams(params(), descendants=300)).run()
	open_message = calldata.decode(calls.pop())['EmitInternalMessage']
	assert open_message['descendants'] == 300

	root = InternalAllocation(Address.ZERO, None, params(), 300)
	proxy.emit(UseBalanceParams(params(), descendants=[root])).run()
	pinned_message = calldata.decode(calls.pop())['EmitInternalMessage']
	assert pinned_message['descendants'][0]['call_key'] == ANY_METHOD
	assert pinned_message['descendants'][0]['parent_index'] == 2**256 - 1


def test_deploy_forwards_descendants(monkeypatch):
	calls = []
	monkeypatch.setattr(
		contract, 'wasi', types.SimpleNamespace(gl_call=calls.append), raising=False
	)

	contract.deploy(
		UseBalanceParams(params(), descendants=300),
		code=b'code',
	)

	message = calldata.decode(calls.pop())['EmitInternalDeployMessage']
	assert message['descendants'] == 300


def test_bool_descendants_are_rejected(monkeypatch):
	calls = []
	monkeypatch.setattr(
		contract, 'wasi', types.SimpleNamespace(gl_call=calls.append), raising=False
	)

	with pytest.raises(TypeError, match='bool'):
		contract.get_at(Address.ZERO).emit(
			UseBalanceParams(params(), descendants=False)
		).run()

	with pytest.raises(TypeError, match='bool'):
		contract.deploy(UseBalanceParams(params(), descendants=False), code=b'code')
	assert calls == []


@pytest.mark.parametrize('keyword', [False, True])
@pytest.mark.parametrize(
	'kind', ['call', 'transfer', 'deploy', 'account_transfer', 'self_transfer']
)
def test_balance_parameter_can_be_positional_or_keyword(monkeypatch, keyword, kind):
	calls = []
	monkeypatch.setattr(
		contract, 'wasi', types.SimpleNamespace(gl_call=calls.append), raising=False
	)
	balance = UseBalanceParams(params())
	args, kwargs = ((), {'use_balance': balance}) if keyword else ((balance,), {})
	proxy = contract.get_at(Address.ZERO)
	if kind == 'call':
		proxy.emit(*args, **kwargs).run()
	elif kind == 'transfer':
		proxy.emit_transfer(1, *args, **kwargs)
	elif kind == 'account_transfer':
		gl.chain.Account(Address.ZERO).emit_transfer(1, *args, **kwargs)
	elif kind == 'self_transfer':
		monkeypatch.setattr(gl.message, 'contract_address', Address.ZERO, raising=False)
		with pytest.warns(UserWarning, match='transfer to self'):
			contract.Contract().emit_transfer(1, *args, **kwargs)
	else:
		assert contract.deploy(*args, code=b'code', **kwargs) is None
	message = next(iter(calldata.decode(calls.pop()).values()))
	assert message['use_balance'] is True
	assert message['fee_params']['rotations'] == [0]
	assert 'descendants' not in message


def test_sender_funding_omits_balance_fields(monkeypatch):
	calls = []
	monkeypatch.setattr(
		contract, 'wasi', types.SimpleNamespace(gl_call=calls.append), raising=False
	)
	contract.get_at(Address.ZERO).emit().run()
	message = calldata.decode(calls.pop())['EmitInternalMessage']
	assert not {'use_balance', 'fee_params', 'descendants'} & message.keys()
	with pytest.raises(TypeError, match='UseBalanceParams'):
		contract.get_at(Address.ZERO).emit(True).run()


def test_descendant_tree_flattens_in_preorder_without_mutation(monkeypatch):
	calls = []
	monkeypatch.setattr(
		contract, 'wasi', types.SimpleNamespace(gl_call=calls.append), raising=False
	)
	leaf = InternalAllocation(Address.ZERO, 'leaf', params(), 10)
	child = InternalAllocation(Address.ZERO, 'child', params(), 20, children=[leaf])
	root = InternalAllocation(Address.ZERO, 'root', params(), 30, children=[child, leaf])
	external = ExternalAllocation(
		Address.ZERO, None, gl.chain.ExternalMessageParams(1, 1), 1
	)
	balance = UseBalanceParams(params(), [root, external])
	for _ in range(2):
		contract.get_at(Address.ZERO).emit(balance).run()
		flat = calldata.decode(calls.pop())['EmitInternalMessage']['descendants']
		assert [node['parent_index'] for node in flat] == [2**256 - 1, 0, 1, 0, 2**256 - 1]
		assert all('children' not in node for node in flat)
	assert root.children == [child, leaf]


def test_cyclic_descendant_tree_is_bounded(monkeypatch):
	calls = []
	monkeypatch.setattr(
		contract, 'wasi', types.SimpleNamespace(gl_call=calls.append), raising=False
	)
	root = InternalAllocation(Address.ZERO, None, params(), 1)
	root.children.append(root)
	with pytest.raises(ValueError, match='1024'):
		contract.get_at(Address.ZERO).emit(UseBalanceParams(params(), [root])).run()
	assert calls == []
