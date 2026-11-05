#!/usr/bin/env python3

import sys
import typing
from pathlib import Path

import eth_abi.abi as eth
import genlayer.evm as genvm_eth
from genlayer.types import Address, SizedArray, i8, i32, i256, u8, u64, u256

sys.path.append(str(Path(__file__).parent.parent))
from fuzz_common import FuzzerBuilder, StopFuzzingException, do_fuzzing

# the value for genlayer.evm and the same value for eth_abi
type Value = tuple[typing.Any, typing.Any]


def _int(builder: FuzzerBuilder, size: int, *, signed: bool = False) -> Value:
	v = int.from_bytes(builder.fetch(size), signed=signed)
	return v, v


def _bool(builder: FuzzerBuilder) -> Value:
	v = builder.fetch(1)[0] % 2 == 1
	return v, v


def _address(builder: FuzzerBuilder) -> Value:
	a = Address(builder.fetch(20))
	return a, a.as_hex


def _bytes(builder: FuzzerBuilder) -> Value:
	v = builder.fetch(builder.fetch(1)[0])
	return v, v


def _str(builder: FuzzerBuilder) -> Value:
	v = builder.fetch_str()
	return v, v


def _fixed_bytes(size: int):
	def make(builder: FuzzerBuilder) -> Value:
		v = builder.fetch(size)
		return v, v

	return make


class Type(typing.NamedTuple):
	py: typing.Any
	eth: str
	value: typing.Callable[[FuzzerBuilder], Value]


LEAVES = [
	Type(u8, 'uint8', lambda b: _int(b, 1)),
	Type(u64, 'uint64', lambda b: _int(b, 8)),
	Type(u256, 'uint256', lambda b: _int(b, 32)),
	Type(i32, 'int32', lambda b: _int(b, 4, signed=True)),
	Type(bool, 'bool', _bool),
	Type(Address, 'address', _address),
	Type(bytes, 'bytes', _bytes),
	Type(str, 'string', _str),
	Type(genvm_eth.bytes3, 'bytes3', _fixed_bytes(3)),
	Type(genvm_eth.bytes32, 'bytes32', _fixed_bytes(32)),
	Type(i8, 'int8', lambda b: _int(b, 1, signed=True)),
	Type(i256, 'int256', lambda b: _int(b, 32, signed=True)),
]


def create_type(builder: FuzzerBuilder, depth: int) -> Type:
	kind = builder.fetch(1)[0]
	if depth == 0 or kind % 4 == 0:
		return LEAVES[(kind // 4) % len(LEAVES)]
	if kind % 4 == 1:
		members = [create_type(builder, depth - 1) for _ in range(kind // 4 % 3 + 1)]

		def tuple_value(b: FuzzerBuilder) -> Value:
			vals = [m.value(b) for m in members]
			return tuple(v for v, _ in vals), tuple(v for _, v in vals)

		return Type(
			tuple[tuple(m.py for m in members)],
			'(' + ','.join(m.eth for m in members) + ')',
			tuple_value,
		)

	elem = create_type(builder, depth - 1)

	def seq_value(b: FuzzerBuilder, count: int) -> Value:
		vals = [elem.value(b) for _ in range(count)]
		return [v for v, _ in vals], [v for _, v in vals]

	if kind % 4 == 2:
		return Type(
			list[elem.py], elem.eth + '[]', lambda b: seq_value(b, b.fetch(1)[0] % 4)
		)
	size = kind // 4 % 3 + 1
	return Type(
		SizedArray[elem.py, typing.Literal[size]],
		f'{elem.eth}[{size}]',
		lambda b: seq_value(b, size),
	)


def evm_encoding(buf):
	builder = FuzzerBuilder(buf)

	try:
		params = [create_type(builder, 3) for _ in range(builder.fetch(1)[0] % 3 + 1)]
		vals = [p.value(builder) for p in params]
	except StopFuzzingException:
		return

	names = [p.eth for p in params]
	ours = tuple(v for v, _ in vals)
	expected = eth.encode(names, [v for _, v in vals])

	enc = genvm_eth.MethodEncoder('f', tuple(p.py for p in params), type(None))
	assert enc.encode_call(ours)[4:] == expected, (names, ours)

	single = eth.encode(names[:1], [vals[0][1]])
	assert genvm_eth.encode(params[0].py, ours[0]) == single, (names[0], ours[0])
	assert genvm_eth.decode(params[0].py, single) == ours[0], (names[0], ours[0])


if __name__ == '__main__':
	do_fuzzing(evm_encoding)
