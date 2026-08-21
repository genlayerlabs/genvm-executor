import pytest
from genlayer.types.keccak import Keccak256, KeccakHash


def test_keccak256_known_digest():
	assert (
		Keccak256(b'').hexdigest()
		== 'c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470'
	)


@pytest.mark.parametrize(
	'args',
	[
		((0, 1600, 256)),
		((8, 17, 8)),
		((8, 192, 8)),
		((1087, 513, 256)),
		((1088, -1, 256)),
		((1088, 511, 256)),
		((1088, 512, 0)),
		((1088, 512, 255)),
	],
)
def test_keccak_rejects_invalid_parameters(args):
	with pytest.raises(ValueError):
		KeccakHash(*args)
