import typing

import eth_abi.abi as eth
import genlayer.evm as genvm_eth
import pytest
from genlayer.types import Address, SizedArray, i32, u8, u256


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
