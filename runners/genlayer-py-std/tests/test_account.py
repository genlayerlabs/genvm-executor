import sys
import types

import genlayer.chain as chain
import genlayer.contract as contract
import pytest
from genlayer import calldata
from genlayer.message_allocation import UseBalanceParams
from genlayer.types import Address

RECIPIENT = Address(b'\x01' * 20)


@pytest.fixture
def emissions(monkeypatch):
	calls = []

	def emit(data):
		calls.append(calldata.decode(data))
		return 2**32 - 1

	monkeypatch.setitem(sys.modules, '_genlayer_wasi', types.ModuleType('_genlayer_wasi'))
	monkeypatch.setattr(chain.gl_call, '_imp_raw', emit, raising=False)
	return calls


@pytest.mark.parametrize('value', [1, 7, 2**256 - 1])
def test_account_transfer_emits_external_empty_calldata(emissions, value):
	assert chain.Account(RECIPIENT).emit_transfer(value) is None
	assert emissions == [
		{'EmitExternalMessage': {'address': RECIPIENT, 'calldata': b'', 'value': value}}
	]


def test_account_transfer_rejects_zero_recipient_before_encoding(
	monkeypatch, emissions
):
	def unexpected(*args, **kwargs):
		pytest.fail('zero-address transfer must fail before encoding or calling the VM')

	monkeypatch.setattr(calldata, 'encode', unexpected)
	monkeypatch.setattr(chain.gl_call, '_imp_raw', unexpected, raising=False)
	with pytest.raises(ValueError, match='zero address'):
		chain.Account(Address.ZERO).emit_transfer(1)
	assert emissions == []


@pytest.mark.parametrize('value', [0, -1])
def test_account_transfer_rejects_nonpositive_value(emissions, value):
	with pytest.raises(ValueError, match='value must be greater than 0'):
		chain.Account(RECIPIENT).emit_transfer(value)
	assert emissions == []


@pytest.mark.parametrize('on', ['decided', 'finalized'])
def test_account_transfer_has_no_stage_parameter(emissions, on):
	with pytest.raises(TypeError, match='on'):
		chain.Account(RECIPIENT).emit_transfer(1, on=on)
	assert emissions == []


@pytest.mark.parametrize('positional', [False, True])
@pytest.mark.parametrize('funded', [False, True])
def test_account_transfer_has_no_balance_funding_parameter(
	emissions, positional, funded
):
	funding = (
		UseBalanceParams(chain.InternalMessageParams(5, 10, 100, [0], 1, 1, 1))
		if funded
		else None
	)
	args, kwargs = ((funding,), {}) if positional else ((), {'use_balance': funding})
	with pytest.raises(TypeError):
		chain.Account(RECIPIENT).emit_transfer(1, *args, **kwargs)
	assert emissions == []


@pytest.mark.parametrize(
	'ic_type', [chain.IAccount, contract.Proxy, contract._ContractAt, contract.Contract]
)
def test_ic_types_have_no_transfer_helper(ic_type):
	assert not hasattr(ic_type, 'emit_transfer')


def test_contract_proxy_has_no_transfer_helper():
	assert not hasattr(contract.get_at(RECIPIENT), 'emit_transfer')
