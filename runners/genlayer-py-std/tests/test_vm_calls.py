import sys
import types

import genlayer.calldata as calldata
import genlayer.contract as contract
import genlayer.vm as vm
import pytest
from genlayer.types import Address, Lazy
from genlayer.vm.public_abi import ResultCode, StorageView


@pytest.fixture(autouse=True)
def cloudpickle_stub(monkeypatch):
	module = types.ModuleType('cloudpickle')
	module.dumps = lambda _value: b'pickled'
	monkeypatch.setitem(sys.modules, 'cloudpickle', module)


def _result(code: ResultCode, value) -> bytes:
	return bytes([code]) + calldata.encode(value)


def _fake_gl_call(payload: bytes):
	def call(_request, decoder):
		return Lazy(lambda: decoder(payload))

	return call


@pytest.mark.parametrize('name', ['run_nondet', 'run_nondet_default'])
def test_run_nondet_catches_only_vm_errors(monkeypatch, name):
	fn = getattr(vm, name)
	vm_error = bytes([ResultCode.VM_ERROR]) + b'exit_code 3'
	monkeypatch.setattr(vm.gl_call, 'gl_call_generic', _fake_gl_call(vm_error))

	result = fn(lambda: 42, lambda _result: True, catch_vm_error=True)
	assert isinstance(result, vm.VMError)
	assert result.message == 'exit_code 3'

	with pytest.raises(vm.UserError, match='vm error: exit_code 3'):
		fn(lambda: 42, lambda _result: True)


@pytest.mark.parametrize('name', ['run_nondet', 'run_nondet_default'])
def test_run_nondet_caught_path_still_unpacks_returns_and_raises_user_errors(
	monkeypatch, name
):
	fn = getattr(vm, name)
	monkeypatch.setattr(
		vm.gl_call,
		'gl_call_generic',
		_fake_gl_call(_result(ResultCode.RETURN, 42)),
	)
	assert fn(lambda: 42, lambda _result: True, catch_vm_error=True) == 42

	monkeypatch.setattr(
		vm.gl_call,
		'gl_call_generic',
		_fake_gl_call(_result(ResultCode.USER_ERROR, 'nope')),
	)
	with pytest.raises(vm.UserError) as exc:
		fn(lambda: 42, lambda _result: True, catch_vm_error=True)
	assert exc.value.data == 'nope'


def test_contract_view_catches_only_vm_errors(monkeypatch):
	vm_error = bytes([ResultCode.VM_ERROR]) + b'exit_code 3'
	monkeypatch.setattr(contract, 'gl_call_generic', _fake_gl_call(vm_error))
	method = contract._ContractAtViewMethod(
		'boom', Address.ZERO, StorageView.LATEST_DECIDED, True
	)
	result = method()
	assert isinstance(result, vm.VMError)
	assert result.message == 'exit_code 3'

	method = contract._ContractAtViewMethod(
		'boom', Address.ZERO, StorageView.LATEST_DECIDED, False
	)
	with pytest.raises(vm.UserError, match='vm error: exit_code 3'):
		method()


def test_decode_empty_vm_result():
	with pytest.raises(ValueError, match='empty VM result'):
		vm._decode_sub_vm_result_retn(b'')


def test_runner_id_ops_is_exported():
	assert 'RunnerIDOps' in vm.__all__
