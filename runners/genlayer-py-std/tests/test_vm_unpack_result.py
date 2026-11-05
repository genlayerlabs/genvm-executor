import genlayer.vm as vm
import pytest


def test_unpack_result_returns_value():
	assert vm.unpack_result(vm.Return({'a': 1})) == {'a': 1}


def test_unpack_result_reraises_errors():
	with pytest.raises(vm.UserError):
		vm.unpack_result(vm.UserError('boom'))
	with pytest.raises(vm.UserError, match='vm error: oops'):
		vm.unpack_result(vm.VMError('oops'))


@pytest.mark.parametrize('value', [{'a': 1}, 42, None])
def test_unpack_result_rejects_unwrapped_value(value):
	with pytest.raises(TypeError, match='already return the unwrapped value'):
		vm.unpack_result(value)  # pyright: ignore[reportArgumentType]
