import pytest
from genlayer._internal import create2_address
from genlayer.contract import Salted
from genlayer.types import Address


def test_create2_matches_ghost_factory():
	# GhostFactory on a node conformance devnet
	assert create2_address(
		Address('0x7bc06c482DEAd17c0e297aFbC32f6e63d3846650'),
		bytes.fromhex('01560dfe6cb54e53503bdef98255051fb87df5f08521bdb891314ab83229c3dd'),
		Address('0x1111111111111111111111111111111111111111'),
		7,
	) == Address('0x6a1a36f4ff75c0dd6588c2afe6b3e2067060a57c')


DEPLOYER = Address('0x7bc06c482DEAd17c0e297aFbC32f6e63d3846650')
CODE_HASH = bytes(32)


@pytest.mark.parametrize('salt_nonce', [0, -1, 2**256])
def test_salted_rejects_out_of_range(salt_nonce):
	with pytest.raises(ValueError):
		Salted(salt_nonce)


@pytest.mark.parametrize('salt_nonce', [True, '1', 1.0])
def test_salted_rejects_non_int(salt_nonce):
	with pytest.raises(TypeError):
		Salted(salt_nonce)


def test_salted_rejects_code_hash_without_deployer():
	with pytest.raises(ValueError):
		Salted(1, code_hash=CODE_HASH)


def test_salted_accepts_deployer_only():
	assert Salted(1, deployer=DEPLOYER).code_hash is None


def test_salted_rejects_short_code_hash():
	with pytest.raises(ValueError):
		Salted(1, deployer=DEPLOYER, code_hash=bytes(31))


def test_salted_accepts_bounds():
	assert Salted(1).salt_nonce == 1
	assert (
		Salted(2**256 - 1, deployer=DEPLOYER, code_hash=CODE_HASH).code_hash == CODE_HASH
	)
