import random
import typing

import eth_abi.abi as eth
import genlayer.evm as genvm_eth
import pytest
from genlayer.types import Address, SizedArray, i32, u8, u64, u256


def test_bytes3():
	assert eth.encode(['bytes3'], [b'123']) == genvm_eth.encode(
		genvm_eth.bytes3, genvm_eth.bytes3(b'123')
	)


@pytest.mark.parametrize('val', [True, False])
def test_bool(val: bool):
	assert eth.encode(['bool'], [val]) == genvm_eth.encode(bool, val)


@pytest.mark.parametrize('val', [-1, -5, 2**31 - 1, -(2**31)])
def test_i32(val: int):
	assert eth.encode(['int32'], [val]) == genvm_eth.encode(i32, i32(val))


@pytest.mark.parametrize(
	'val',
	[
		[],
		[([], []), ([], [])],
		[([], [('1', '5')]), ([], [])],
		[([], []), ([('2', '3')], [])],
		[([('1', '2')], [('3', '4')]), ([('6', '7'), ('8', '9')], [('0000', '1111')])],
		[
			([('1', '2')], [('3', '4')]),
			([('6', '7'), ('8', '9')], [('0000' * 64, '1111' * 64)] * 40),
		],
	],
)
def test_list_tuple_list_tuple_str(
	val: list[tuple[list[tuple[str, str]], list[tuple[str, str]]]],
):
	typ = list[tuple[list[tuple[str, str]], list[tuple[str, str]]]]
	enc = genvm_eth.MethodEncoder('', (typ,), type(None))
	assert eth.encode([enc._encoder.name[1:-1]], [val]) == enc.encode_call((val,))[4:]

	assert genvm_eth.decode(typ, genvm_eth.encode(typ, val)) == val


@pytest.mark.parametrize(
	'val',
	[
		['', '', ''],
		['1', '2', '3'],
		['1', '2' * 128, '3'],
		['1' * 128, '2', '3'],
	],
)
def test_sized_array(val: list[str]):
	typ = SizedArray[str, typing.Literal[3]]
	exp = eth.encode(['string[3]'], [val])
	got = genvm_eth.encode(typ, val)

	assert exp == got

	assert genvm_eth.decode(typ, genvm_eth.encode(typ, val)) == val


@pytest.mark.parametrize(
	'val',
	[
		(('', ''), ('', '')),
		(('a', 'b'), ('c', 'd')),
		(('a' * 100, 'b'), ('c', 'd' * 100)),
	],
)
def test_tuple_tuple_str(val: tuple[tuple[str, str], tuple[str, str]]):
	typ = tuple[tuple[str, str], tuple[str, str]]
	enc = genvm_eth.MethodEncoder('', (typ,), type(None))
	assert eth.encode([enc._encoder.name[1:-1]], [val]) == enc.encode_call((val,))[4:]

	assert genvm_eth.decode(typ, genvm_eth.encode(typ, val)) == val


@pytest.mark.parametrize(
	'val',
	[
		(['', ''], ['', '', '']),
		(['a', 'b'], ['c', 'd', 'e']),
		(['a' * 100, 'b'], ['c', 'd' * 100, 'e']),
	],
)
def test_tuple_sized_str(
	val: tuple[SizedArray[str, typing.Literal[2]], SizedArray[str, typing.Literal[3]]],
):
	typ = tuple[SizedArray[str, typing.Literal[2]], SizedArray[str, typing.Literal[3]]]
	enc = genvm_eth.MethodEncoder(
		'',
		(typ,),
		type(None),
	)
	assert eth.encode([enc._encoder.name[1:-1]], [val]) == enc.encode_call((val,))[4:]

	assert genvm_eth.decode(typ, genvm_eth.encode(typ, val)) == val


ALICE = Address('0x5B38Da6a701c568545dCfcB03FcB875f56beddC4')
BOB = Address('0xAb8483F64d9C6d1EcF9b849Ae677dD3315835cb2')


@pytest.mark.parametrize(
	'types, eth_types, args, eth_args',
	[
		pytest.param(
			# the case from the report: a struct argument followed by another argument
			(tuple[Address, u256], u256),
			['(address,uint256)', 'uint256'],
			((ALICE, 10**18), 1700000000),
			[(ALICE.as_hex, 10**18), 1700000000],
			id='static_tuple_then_uint',
		),
		pytest.param(
			(u8, tuple[u8, u8], u8),
			['uint8', '(uint8,uint8)', 'uint8'],
			(1, (2, 3), 4),
			[1, (2, 3), 4],
			id='uint_static_tuple_uint',
		),
		pytest.param(
			(tuple[u8, u8], tuple[u8, u8]),
			['(uint8,uint8)', '(uint8,uint8)'],
			((1, 2), (3, 4)),
			[(1, 2), (3, 4)],
			id='two_static_tuples',
		),
		pytest.param(
			(tuple[tuple[u8, u8], u8], u8),
			['((uint8,uint8),uint8)', 'uint8'],
			(((1, 2), 3), 4),
			[((1, 2), 3), 4],
			id='nested_static_tuple_then_uint',
		),
		pytest.param(
			(SizedArray[tuple[u8, u8], typing.Literal[2]], u8),
			['(uint8,uint8)[2]', 'uint8'],
			([(1, 2), (3, 4)], 5),
			[[(1, 2), (3, 4)], 5],
			id='fixed_array_of_static_tuples_then_uint',
		),
		pytest.param(
			(list[tuple[tuple[u8, u8], u8]],),
			['((uint8,uint8),uint8)[]'],
			([((1, 2), 3), ((4, 5), 6)],),
			[[((1, 2), 3), ((4, 5), 6)]],
			id='dynamic_array_of_tuples_with_static_tuple',
		),
		pytest.param(
			(tuple[u8, u8], str, u8),
			['(uint8,uint8)', 'string', 'uint8'],
			((1, 2), 'dynamic', 3),
			[(1, 2), 'dynamic', 3],
			id='static_tuple_dynamic_then_static',
		),
		pytest.param(
			(tuple[tuple[u8, u8], u8, str], u8),
			['((uint8,uint8),uint8,string)', 'uint8'],
			(((1, 2), 3, 'dynamic'), 4),
			[((1, 2), 3, 'dynamic'), 4],
			id='static_tuple_inside_dynamic_tuple',
		),
		pytest.param(
			(tuple[bool, Address, genvm_eth.bytes3], u256),
			['(bool,address,bytes3)', 'uint256'],
			((True, BOB, genvm_eth.bytes3(b'abc')), 7),
			[(True, BOB.as_hex, b'abc'), 7],
			id='mixed_static_members',
		),
	],
)
def test_static_tuple_arguments_match_eth_abi(types, eth_types, args, eth_args):
	enc = genvm_eth.MethodEncoder('f', types, type(None))
	assert enc.encode_call(args)[4:] == eth.encode(eth_types, eth_args)


def test_static_tuple_is_written_in_place():
	enc = genvm_eth.MethodEncoder('f', (tuple[u8, u8], u8), type(None))
	data = enc.encode_call(((1, 2), 3))[4:]
	words = [int.from_bytes(data[i : i + 32], 'big') for i in range(0, len(data), 32)]
	# the head is exactly: tuple member 1, tuple member 2, then the next parameter
	assert words == [1, 2, 3]


@pytest.mark.parametrize(
	'typ, eth_type, val, eth_val',
	[
		pytest.param(
			tuple[tuple[u8, u8], u8],
			'((uint8,uint8),uint8)',
			((1, 2), 3),
			((1, 2), 3),
			id='nested_static_tuple',
		),
		pytest.param(
			tuple[tuple[bool, u8], u256],
			'((bool,uint8),uint256)',
			((True, 9), 10**30),
			((True, 9), 10**30),
			id='static_tuple_then_big_uint',
		),
	],
)
def test_static_tuple_standalone_encode_and_roundtrip(typ, eth_type, val, eth_val):
	encoded = genvm_eth.encode(typ, val)
	assert encoded == eth.encode([eth_type], [eth_val])
	assert genvm_eth.decode(typ, encoded) == val


_FUZZ_LEAVES = ('u8', 'u64', 'u256', 'i32', 'bool', 'address', 'bytes', 'str')


def _fuzz_type(r: random.Random, depth: int):
	"""('leaf', name) | ('tuple', [..]) | ('list', t) | ('array', t, n)"""
	if depth <= 0 or r.random() < 0.4:
		return ('leaf', r.choice(_FUZZ_LEAVES))
	kind = r.choice(['tuple', 'tuple', 'list', 'array'])
	if kind == 'tuple':
		return ('tuple', [_fuzz_type(r, depth - 1) for _ in range(r.randint(1, 3))])
	if kind == 'list':
		return ('list', _fuzz_type(r, depth - 1))
	return ('array', _fuzz_type(r, depth - 1), r.randint(1, 3))


def _fuzz_py_type(t):
	if t[0] == 'leaf':
		return {
			'u8': u8,
			'u64': u64,
			'u256': u256,
			'i32': i32,
			'bool': bool,
			'address': Address,
			'bytes': bytes,
			'str': str,
		}[t[1]]
	if t[0] == 'tuple':
		return tuple[tuple(_fuzz_py_type(x) for x in t[1])]
	if t[0] == 'list':
		return list[_fuzz_py_type(t[1])]
	return SizedArray[_fuzz_py_type(t[1]), typing.Literal[t[2]]]


def _fuzz_eth_type(t) -> str:
	if t[0] == 'leaf':
		return {
			'u8': 'uint8',
			'u64': 'uint64',
			'u256': 'uint256',
			'i32': 'int32',
			'bool': 'bool',
			'address': 'address',
			'bytes': 'bytes',
			'str': 'string',
		}[t[1]]
	if t[0] == 'tuple':
		return '(' + ','.join(_fuzz_eth_type(x) for x in t[1]) + ')'
	if t[0] == 'list':
		return _fuzz_eth_type(t[1]) + '[]'
	return _fuzz_eth_type(t[1]) + f'[{t[2]}]'


def _fuzz_value(r: random.Random, t):
	"""returns the value for genlayer.evm and the same value for eth_abi"""
	if t[0] == 'leaf':
		match t[1]:
			case 'u8':
				v = r.randrange(2**8)
			case 'u64':
				v = r.randrange(2**64)
			case 'u256':
				v = r.randrange(2**256)
			case 'i32':
				v = r.randrange(-(2**31), 2**31)
			case 'bool':
				v = r.random() < 0.5
			case 'address':
				a = Address(bytes(r.randrange(256) for _ in range(20)))
				return a, a.as_hex
			case 'bytes':
				v = bytes(r.randrange(256) for _ in range(r.choice([0, 1, 32, 33])))
			case _:
				v = ''.join(r.choice('abc') for _ in range(r.choice([0, 1, 32, 33])))
		return v, v
	if t[0] == 'tuple':
		vs = [_fuzz_value(r, x) for x in t[1]]
		return tuple(a for a, _ in vs), tuple(b for _, b in vs)
	count = r.randint(0, 3) if t[0] == 'list' else t[2]
	vs = [_fuzz_value(r, t[1]) for _ in range(count)]
	return [a for a, _ in vs], [b for _, b in vs]


def test_random_types_match_eth_abi():
	# fixed seed: the generated cases are the same on every run
	r = random.Random(20260101)
	for _ in range(400):
		params = [_fuzz_type(r, 3) for _ in range(r.randint(1, 3))]
		vals = [_fuzz_value(r, p) for p in params]
		names = [_fuzz_eth_type(p) for p in params]

		enc = genvm_eth.MethodEncoder(
			'f', tuple(_fuzz_py_type(p) for p in params), type(None)
		)
		got = enc.encode_call(tuple(a for a, _ in vals))[4:]
		assert got == eth.encode(names, [b for _, b in vals]), names

		t = _fuzz_type(r, 3)
		a, b = _fuzz_value(r, t)
		assert genvm_eth.encode(_fuzz_py_type(t), a) == eth.encode(
			[_fuzz_eth_type(t)], [b]
		), _fuzz_eth_type(t)
