import genlayer.calldata as calldata

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
