import array

import genlayer.calldata as calldata
import numpy as np
import pytest

# Shared cross-language corpus: (logical value, expected canonical hex). The exact same
# hex is pinned in the Rust encoder test (crates/calldata/src/bin.rs) so the two encoders
# can never silently diverge. Covers boundary ints, empty/short str+bytes, a map whose
# content-order ("aa" < "z") differs from length-order, and nested containers.
CORPUS = [
	(0, '01'),
	(-1, '02'),
	(2**64, '81808080808080808010'),
	(-(2**64), 'faffffffffffffffff0f'),
	('', '04'),
	('hello', '2c68656c6c6f'),
	(bytes([1, 2, 3]), '1b010203'),
	({'z': 1, 'aa': 2}, '1602616111017a09'),
	({'': None, 'a': [1, 2, {'b': False}]}, '16000001611d09110e016208'),
]


def test_calldata_corpus_encode():
	for value, expected in CORPUS:
		assert calldata.encode(value).hex() == expected, f'encoding mismatch for {value!r}'


def test_calldata_corpus_decode_roundtrip():
	for value, expected in CORPUS:
		assert calldata.decode(bytes.fromhex(expected)) == value, (
			f'roundtrip mismatch for {value!r}'
		)


def test_memoryview_roundtrip_as_bytes():
	value = memoryview(b'abc')
	assert calldata.decode(calldata.encode(value)) == value.tobytes()


def test_buffer_roundtrip_as_bytes():
	value = array.array('I', [0x12345678])
	assert calldata.decode(calldata.encode(value)) == value.tobytes()


class _IndexBuffer:
	# a buffer that is also usable as an index, like a numpy integer scalar
	def __buffer__(self, flags: int) -> memoryview:
		return memoryview(b'\x07\x00')

	def __index__(self) -> int:
		return 3


@pytest.mark.parametrize(
	'value',
	[
		_IndexBuffer(),
		np.uint8(3),
		np.uint32(70000),
		np.int8(-1),
		np.uint64(2**40),
		np.array(7),
	],
	ids=[
		'index_buffer',
		'uint8',
		'uint32',
		'negative_int8',
		'large_uint64',
		'zero_dim_array',
	],
)
def test_index_buffer_roundtrips_as_its_bytes(value):
	assert calldata.decode(calldata.encode(value)) == bytes(memoryview(value))
