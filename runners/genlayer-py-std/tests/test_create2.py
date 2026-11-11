import pickle
import sys
import types

import genlayer._internal as internal
import genlayer.calldata as calldata
import genlayer.contract as contract
import genlayer.message as message
import pytest
from genlayer._internal import create2_address, predict_ghost_address
from genlayer.contract import Salted
from genlayer.types import Address, Keccak256, Lazy


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
PARENT = Address(b'\x11' * 20)
PREDICTED = Address(b'\x22' * 20)
OVERRIDE = Address(b'\x33' * 20)


@pytest.fixture(autouse=True)
def create2_context(monkeypatch):
	monkeypatch.setattr(
		message, 'create2', {'deployer': DEPLOYER, 'code_hash': CODE_HASH}
	)
	monkeypatch.setattr(message, 'contract_address', PARENT)

	def unexpected(*args):
		pytest.fail('unexpected chain call')

	monkeypatch.setattr(internal, 'predict_ghost_address', unexpected)


@pytest.fixture
def predictions(monkeypatch):
	calls = []

	def predict(*args):
		calls.append(args)
		return PREDICTED

	monkeypatch.setattr(internal, 'predict_ghost_address', predict)
	return calls


@pytest.fixture
def emissions(monkeypatch):
	calls = []
	monkeypatch.setattr(
		contract,
		'wasi',
		types.SimpleNamespace(gl_call=lambda data: calls.append(calldata.decode(data))),
		raising=False,
	)
	return calls


@pytest.mark.parametrize(
	'overrides',
	[{}, {'deployer': OVERRIDE}, {'deployer': OVERRIDE, 'code_hash': b'\x42' * 32}],
)
def test_salted_constructor_defaults_without_chain_calls(overrides):
	salted = Salted(7, **overrides)
	assert salted.deployer == overrides.get('deployer', DEPLOYER)
	assert salted.code_hash == overrides.get('code_hash', CODE_HASH)
	assert salted._address is None
	assert not hasattr(salted, '__dict__')
	assert '_address=None' in repr(salted)


@pytest.mark.parametrize('cached', ['none', 'resolve', 'resolve_hardcoded'])
def test_salted_pickle_drops_prediction_cache(monkeypatch, predictions, cached):
	salted = Salted(7, deployer=OVERRIDE, code_hash=b'\x42' * 32)
	if cached != 'none':
		getattr(salted, cached)()
	original_address = salted._address
	serialized = pickle.dumps(salted)
	copied = pickle.loads(serialized)
	assert copied.salt_nonce == salted.salt_nonce
	assert copied.deployer == salted.deployer
	assert copied.code_hash == salted.code_hash
	assert copied._address is None
	assert salted._address == original_address
	if original_address is not None:
		assert original_address.as_bytes not in serialized

	calls = []

	def changed_factory(*args):
		calls.append(args)
		return Address(b'\x55' * 20)

	monkeypatch.setattr(internal, 'predict_ghost_address', changed_factory)
	assert copied.resolve() == Address(b'\x55' * 20)
	assert copied.resolve() == Address(b'\x55' * 20)
	assert calls == [(OVERRIDE, 7, PARENT)]


@pytest.mark.parametrize('resolved', [False, True])
@pytest.mark.parametrize('nested', [False, True])
def test_salted_is_not_calldata_encodable(resolved, nested):
	salted = Salted(7)
	if resolved:
		salted.resolve_hardcoded()
	with pytest.raises(TypeError):
		calldata.encode({'salted': salted} if nested else salted)


def test_salted_inputs_cannot_diverge_from_cached_deployment(emissions):
	salted = Salted(7)
	address = salted.resolve_hardcoded()
	for field, value in [
		('salt_nonce', 8),
		('deployer', OVERRIDE),
		('code_hash', b'\x42' * 32),
	]:
		with pytest.raises(AttributeError):
			setattr(salted, field, value)
		with pytest.raises(AttributeError):
			delattr(salted, field)
	assert contract.deploy(code=b'code', salted=salted) == address
	assert emissions[0]['EmitInternalDeployMessage']['salt_nonce'] == 7


@pytest.mark.parametrize('deployer', [None, OVERRIDE])
def test_salted_resolve_calls_factory_once_and_caches(predictions, deployer):
	salted = Salted(7, deployer=deployer)
	assert predictions == []
	assert salted.resolve() == PREDICTED
	assert salted.resolve() == PREDICTED
	assert predictions == [(deployer or DEPLOYER, 7, PARENT)]
	assert f'_address={PREDICTED!r}' in repr(salted)


@pytest.mark.parametrize(
	'overrides', [{}, {'deployer': OVERRIDE, 'code_hash': b'\x42' * 32}]
)
def test_hardcoded_resolution_caches_without_chain_calls(overrides):
	salted = Salted(7, **overrides)
	expected = create2_address(salted.deployer, salted.code_hash, PARENT, 7)
	assert salted.resolve_hardcoded() == expected
	assert salted.resolve_hardcoded() == expected
	assert salted.resolve() == expected


@pytest.mark.parametrize('matches', [False, True])
def test_hardcoded_resolution_checks_stored_address(monkeypatch, matches):
	salted = Salted(7)
	hardcoded = create2_address(DEPLOYER, CODE_HASH, PARENT, 7)
	stored = hardcoded if matches else PREDICTED
	monkeypatch.setattr(internal, 'predict_ghost_address', lambda *args: stored)
	assert salted.resolve() == stored
	if matches:
		assert salted.resolve_hardcoded() == stored
	else:
		with pytest.raises(ValueError) as exc:
			salted.resolve_hardcoded()
		assert str(hardcoded) in str(exc.value)
		assert str(stored) in str(exc.value)
	assert salted.resolve() == stored


def test_resolve_failure_is_not_cached(monkeypatch):
	salted = Salted(7)

	def fail(*args):
		raise ValueError('factory rejected prediction')

	monkeypatch.setattr(internal, 'predict_ghost_address', fail)
	with pytest.raises(ValueError, match='factory rejected'):
		salted.resolve()
	assert salted._address is None
	monkeypatch.setattr(internal, 'predict_ghost_address', lambda *args: PREDICTED)
	assert salted.resolve() == PREDICTED


@pytest.mark.parametrize('salted', [None, 1, 2**256 - 1])
def test_deploy_without_prediction(monkeypatch, emissions, salted):
	monkeypatch.setattr(message, 'create2', None)
	assert contract.deploy(code=b'code', salted=salted) is None
	assert emissions == [
		{
			'EmitInternalDeployMessage': {
				'calldata': {},
				'code': b'code',
				'value': 0,
				'on': 'finalized',
				'salt_nonce': salted or 0,
			}
		}
	]


@pytest.mark.parametrize('cached', ['none', 'resolve', 'resolve_hardcoded'])
def test_deploy_salted_resolves_address(emissions, predictions, cached):
	salted = Salted(7)
	if cached != 'none':
		getattr(salted, cached)()
	expected = (
		create2_address(DEPLOYER, CODE_HASH, PARENT, 7)
		if cached == 'resolve_hardcoded'
		else PREDICTED
	)
	assert (
		contract.deploy(
			code=b'code', args=[1], kwargs={'a': 2}, salted=salted, value=3, on='decided'
		)
		== expected
	)
	assert predictions == (
		[] if cached == 'resolve_hardcoded' else [(DEPLOYER, 7, PARENT)]
	)
	assert emissions == [
		{
			'EmitInternalDeployMessage': {
				'calldata': {'args': [1], 'kwargs': {'a': 2}},
				'code': b'code',
				'value': 3,
				'on': 'decided',
				'salt_nonce': 7,
			}
		}
	]


@pytest.mark.parametrize('salted', [0, -1, 2**256])
def test_deploy_rejects_out_of_range_salt(emissions, salted):
	with pytest.raises(ValueError, match=r'^salted must be in \[1, 2\*\*256\)$'):
		contract.deploy(code=b'code', salted=salted)
	assert emissions == []


@pytest.mark.parametrize('salted', [False, True, '1', 1.0, object()])
def test_deploy_rejects_invalid_salt_types(emissions, salted):
	with pytest.raises(TypeError, match='^salted must be '):
		contract.deploy(code=b'code', salted=salted)
	assert emissions == []


def test_deploy_prediction_failure_emits_nothing(monkeypatch, emissions):
	def fail(*args):
		raise ValueError('predictGhostAddress returned the zero address')

	monkeypatch.setattr(internal, 'predict_ghost_address', fail)
	with pytest.raises(ValueError, match='zero address'):
		contract.deploy(code=b'code', salted=Salted(7))
	assert emissions == []


@pytest.mark.parametrize('predicted', [PREDICTED, Address.ZERO])
def test_resolve_uses_factory_eth_call(monkeypatch, emissions, predicted):
	monkeypatch.setitem(sys.modules, '_genlayer_wasi', types.ModuleType('_genlayer_wasi'))
	from genlayer._internal.on_chain import eth

	calls = []

	def call(request, decoder):
		calls.append(request)
		return Lazy(lambda: decoder(bytes(12) + predicted.as_bytes))

	monkeypatch.setattr(eth.gl_call, 'gl_call_generic', call)
	monkeypatch.setattr(internal, 'predict_ghost_address', predict_ghost_address)
	salted = Salted(7, deployer=OVERRIDE)
	if predicted == Address.ZERO:
		with pytest.raises(ValueError, match='zero address'):
			contract.deploy(code=b'code', salted=salted)
		assert emissions == []
		assert salted._address is None
	else:
		assert contract.deploy(code=b'code', salted=salted) == predicted
		assert salted.resolve() == predicted
	assert calls == [
		{
			'ExternalCall': {
				'address': OVERRIDE,
				'calldata': Keccak256(b'predictGhostAddress(uint256,address)').digest()[:4]
				+ (7).to_bytes(32, 'big')
				+ bytes(12)
				+ PARENT.as_bytes,
			}
		}
	]


@pytest.mark.parametrize('salt_nonce', [0, -1, 2**256])
def test_salted_rejects_out_of_range(salt_nonce):
	with pytest.raises(ValueError, match=r'^salt_nonce must be in \[1, 2\*\*256\)$'):
		Salted(salt_nonce)


@pytest.mark.parametrize('salt_nonce', [True, '1', 1.0])
def test_salted_rejects_non_int(salt_nonce):
	with pytest.raises(TypeError, match='^salt_nonce must be int$'):
		Salted(salt_nonce)


def test_salted_rejects_code_hash_without_deployer():
	with pytest.raises(ValueError):
		Salted(1, code_hash=CODE_HASH)


def test_salted_accepts_deployer_only():
	assert Salted(1, deployer=DEPLOYER).code_hash == CODE_HASH


def test_salted_rejects_short_code_hash():
	with pytest.raises(ValueError):
		Salted(1, deployer=DEPLOYER, code_hash=bytes(31))


def test_salted_accepts_bounds():
	assert Salted(1).salt_nonce == 1
	assert (
		Salted(2**256 - 1, deployer=DEPLOYER, code_hash=CODE_HASH).code_hash == CODE_HASH
	)
