from base64 import b64encode

import pytest
from genlayer.types import Address


@pytest.mark.parametrize(
	'as_str',
	[
		'0x03FB09251eC05ee9Ca36c98644070B89111D4b3F',
		'0x90F8bf6A479f320ead074411a4B0e7944Ea8c9C1',
	],
)
def test_addr(as_str: str):
	addr = Address(as_str.lower())
	assert addr.as_hex == as_str


def test_addr_zero():
	addr = Address.ZERO
	assert addr.as_hex == '0x0000000000000000000000000000000000000000'


def test_addr_constructors():
	origin = '0x03FB09251eC05ee9Ca36c98644070B89111D4b3F'
	origin_bytes = bytes.fromhex(origin[2:])
	addr = Address(origin_bytes)
	assert addr.as_hex == origin

	addr = Address(origin)
	assert addr.as_hex == origin

	addr = Address(b64encode(origin_bytes).decode('ascii'))
	assert addr.as_hex == origin

	addr2 = Address(addr)
	assert addr2.as_hex == origin


def test_addr_as_int_is_big_endian():
	addr = Address('0x03FB09251eC05ee9Ca36c98644070B89111D4b3F')
	assert addr.as_int == int(addr.as_hex, 16)
	assert addr.as_int.to_bytes(Address.SIZE, 'big') == addr.as_bytes


@pytest.mark.parametrize(
	'val',
	[
		'0x01',
		'0xzz00000000000000000000000000000000000000',
		'not base64',
		b'',
		b'\x00' * (Address.SIZE - 1),
	],
)
def test_addr_rejects_invalid_encodings_and_lengths(val):
	with pytest.raises(ValueError):
		Address(val)


@pytest.mark.parametrize('val', [None, 20, object()])
def test_addr_rejects_unsupported_types(val):
	with pytest.raises(TypeError):
		Address(val)


def test_addr_comparison_with_other_types():
	addr = Address.ZERO
	assert addr != 0
	with pytest.raises(TypeError):
		addr < 0
