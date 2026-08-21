import typing

import genlayer.evm as genvm_eth
import pytest
from genlayer.storage import Array
from genlayer.types import Address, i8, u8


def _word(value: int) -> bytes:
	return value.to_bytes(32, 'big')


@pytest.mark.parametrize('typ', [u8, bool, Address, genvm_eth.bytes3])
def test_decode_rejects_short_words(typ: type):
	with pytest.raises(genvm_eth.DecodingError, match='unexpected end'):
		genvm_eth.decode(typ, b'\x00' * 31)


@pytest.mark.parametrize(
	'typ,data',
	[
		(bool, _word(2)),
		(Address, b'\x01' + b'\x00' * 31),
		(genvm_eth.bytes3, b'abc' + b'\x00' * 28 + b'\x01'),
		(u8, _word(256)),
		(i8, _word(255)),
	],
)
def test_decode_rejects_noncanonical_words(typ: type, data: bytes):
	with pytest.raises(genvm_eth.DecodingError):
		genvm_eth.decode(typ, data)


@pytest.mark.parametrize(
	'data,message',
	[
		(_word(33) + b'\x00' * 64, 'not 32-byte aligned'),
		(_word(64), 'outside the data'),
		(_word(32) + _word(64) + b'\x00' * 64, 'points into'),
	],
)
def test_decode_rejects_invalid_dynamic_offsets(data: bytes, message: str):
	typ = str if len(data) < 96 else tuple[genvm_eth.InplaceTuple, str, str]
	with pytest.raises(genvm_eth.DecodingError, match=message):
		genvm_eth.decode(typ, data)


def test_decode_rejects_dynamic_data_out_of_bounds():
	data = _word(32) + _word(33) + b'a' * 32
	with pytest.raises(genvm_eth.DecodingError, match='unexpected end'):
		genvm_eth.decode(bytes, data)


def test_decode_rejects_dynamic_nonzero_padding():
	data = _word(32) + _word(1) + b'a' + b'\x00' * 30 + b'\x01'
	with pytest.raises(genvm_eth.DecodingError, match='non-zero trailing padding'):
		genvm_eth.decode(bytes, data)


def test_decode_wraps_invalid_utf8():
	data = _word(32) + _word(1) + b'\xff' + b'\x00' * 31
	with pytest.raises(genvm_eth.DecodingError, match='invalid UTF-8'):
		genvm_eth.decode(str, data)


def test_decode_rejects_array_head_out_of_bounds():
	data = _word(32) + _word(2) + _word(1)
	with pytest.raises(genvm_eth.DecodingError, match='head is outside'):
		genvm_eth.decode(list[u8], data)


@pytest.mark.parametrize(
	'typ,value',
	[
		(u8, -1),
		(u8, 256),
		(i8, -129),
		(i8, 128),
	],
)
def test_encode_rejects_out_of_range_integers(typ: type, value: int):
	with pytest.raises(ValueError, match='outside the range'):
		genvm_eth.encode(typ, value)


@pytest.mark.parametrize(
	'typ,value',
	[
		(Array[u8, typing.Literal[2]], [1]),
		(Array[u8, typing.Literal[2]], [1, 2, 3]),
		(Array[str, typing.Literal[2]], ['a']),
		(Array[str, typing.Literal[2]], ['a', 'b', 'c']),
		(tuple[u8, u8], (1,)),
		(tuple[u8, u8], (1, 2, 3)),
		(tuple[u8, str], (1,)),
		(tuple[u8, str], (1, 'a', 'b')),
		(genvm_eth.bytes3, b'ab'),
		(genvm_eth.bytes3, b'abcd'),
	],
)
def test_encode_rejects_wrong_fixed_length(typ: type, value: object):
	with pytest.raises(ValueError, match='expected'):
		genvm_eth.encode(typ, value)


def test_decode_continues_to_allow_trailing_data():
	assert genvm_eth.decode(u8, _word(1) + b'trailing') == 1
