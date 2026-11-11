import types

import genlayer.eq_principle as eq_principle
import genlayer.vm as vm
import pytest
from genlayer.types import Address, Lazy


@pytest.mark.parametrize('result_type', [vm.Return, vm.UserError])
@pytest.mark.parametrize(
	'left, right',
	[
		(1, True),
		(0, False),
		([1], [True]),
		({'nested': [{'value': 1}]}, {'nested': [{'value': True}]}),
		(Address.ZERO, Address.ZERO.as_bytes),
	],
)
def test_strict_eq_distinguishes_calldata_types(monkeypatch, result_type, left, right):
	assert _validate(monkeypatch, result_type(left), result_type(right)) is False
	assert _validate(monkeypatch, result_type(right), result_type(left)) is False


@pytest.mark.parametrize('result_type', [vm.Return, vm.UserError])
@pytest.mark.parametrize(
	'left, right',
	[
		(None, None),
		(True, True),
		(1, 1),
		({'a': 1, 'b': [False]}, {'b': [False], 'a': 1}),
	],
)
def test_strict_eq_agrees_on_canonical_values(monkeypatch, result_type, left, right):
	assert _validate(monkeypatch, result_type(left), result_type(right)) is True


@pytest.mark.parametrize(
	'left, right, expected',
	[
		(vm.Return('error'), vm.UserError('error'), False),
		(vm.Return('error'), vm.VMError('error'), False),
		(vm.UserError('error'), vm.VMError('error'), False),
		(vm.VMError('error'), vm.VMError('error'), True),
		(vm.VMError('error'), vm.VMError('other'), False),
	],
)
def test_strict_eq_preserves_result_kinds(monkeypatch, left, right, expected):
	assert _validate(monkeypatch, left, right) is expected
	assert _validate(monkeypatch, right, left) is expected


def _validate(monkeypatch, local, leader):
	def fn():
		pytest.fail('validator must run the function in a sandbox')

	def spawn(function):
		assert function is fn
		return local

	def run(function, validator):
		assert function is fn
		return Lazy(lambda: validator(leader))

	monkeypatch.setattr(vm, 'spawn_sandbox', spawn)
	monkeypatch.setattr(vm, 'run_nondet', types.SimpleNamespace(lazy=run))
	return eq_principle.strict_eq(fn)
