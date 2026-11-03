import typing
from dataclasses import dataclass

import genlayer._internal.get_schema as _get_schema
import pytest
from genlayer._internal.get_schema import get_schema
from genlayer.types import Address, u256


def public(f):
	setattr(f, _get_schema.PUBLIC_ATTR, True)
	return f


def public_view(f):
	setattr(f, _get_schema.PUBLIC_ATTR, True)
	setattr(f, _get_schema.READONLY_ATTR, True)
	return f


class A:
	def __init__(self, x: int, *, y: str): ...

	@public
	def foo(self, x: dict, *, y: list): ...

	@public_view
	def foo_bar(self, x: dict[str, int], *, y: list[list[int]]): ...

	@public_view
	def an(self, x: typing.Any): ...


def test_class():
	print(get_schema(A))
	assert get_schema(A) == {
		'ctor': {'params': [['x', 'int']], 'kwparams': {'y': 'string'}},
		'methods': {
			'an': {
				'params': [['x', 'any']],
				'kwparams': {},
				'readonly': True,
				'ret': 'any',
			},
			'foo': {
				'params': [['x', 'dict']],
				'kwparams': {'y': 'array'},
				'readonly': False,
				'payable': False,
				'ret': 'any',
			},
			'foo_bar': {
				'params': [['x', {'$dict': 'int'}]],
				'kwparams': {'y': [{'$rep': [{'$rep': 'int'}]}]},
				'readonly': True,
				'ret': 'any',
			},
		},
	}


@dataclass
class B_data:
	x: int
	y: int
	z: typing.Literal['str']


class B:
	def __init__(self, x: u256, y: Address):
		pass

	@public_view
	def tst(self) -> B_data: ...


def test_dataclass():
	print(get_schema(B))
	assert get_schema(B) == {
		'ctor': {'params': [['x', 'int'], ['y', 'address']], 'kwparams': {}},
		'methods': {
			'tst': {
				'params': [],
				'kwparams': {},
				'readonly': True,
				'ret': {'x': 'int', 'y': 'int', 'z': 'string'},
			}
		},
	}


def test_literal_bool():
	import genlayer._internal.get_schema as gs

	# typing.Literal[True, False] must be 'bool', not 'int'
	assert gs._repr_type(typing.Literal[True, False], False) == 'bool'
	assert gs._repr_type(typing.Literal[True], False) == 'bool'
	assert gs._repr_type(typing.Literal[False], False) == 'bool'

	# sanity-check: plain int literals still resolve to 'int'
	assert gs._repr_type(typing.Literal[0, 1], False) == 'int'


def public_payable(f):
	setattr(f, _get_schema.PUBLIC_ATTR, True)
	setattr(f, _get_schema.PAYABLE_ATTR, True)
	return f


def test_receive_is_public_but_not_in_schema():
	class Vault:
		def __init__(self): ...

		@public_view
		def total(self) -> int: ...

		@public_payable
		def __receive__(self): ...

	assert list(get_schema(Vault)['methods']) == ['total']


def test_handle_undefined_method_is_public_but_not_in_schema():
	class Proxy:
		def __init__(self): ...

		@public
		def forward(self, x: int): ...

		@public
		def __handle_undefined_method__(
			self, method_name: str, args: list, kwargs: dict
		): ...

	assert list(get_schema(Proxy)['methods']) == ['forward']


def test_both_special_methods_and_nothing_else():
	class OnlySpecial:
		def __init__(self): ...

		@public_payable
		def __receive__(self): ...

		@public
		def __handle_undefined_method__(
			self, method_name: str, args: list, kwargs: dict
		): ...

	assert get_schema(OnlySpecial)['methods'] == {}


def test_special_methods_do_not_change_the_rest_of_the_schema():
	class Plain:
		def __init__(self, x: u256): ...

		@public_view
		def total(self) -> int: ...

	class WithSpecial:
		def __init__(self, x: u256): ...

		@public_view
		def total(self) -> int: ...

		@public_payable
		def __receive__(self): ...

		@public
		def __handle_undefined_method__(
			self, method_name: str, args: list, kwargs: dict
		): ...

	assert get_schema(WithSpecial) == get_schema(Plain)


def test_private_special_methods_are_ignored_as_before():
	class Vault:
		def __init__(self): ...

		@public_view
		def total(self) -> int: ...

		def __receive__(self): ...

		def __handle_undefined_method__(self, method_name, args, kwargs): ...

	assert list(get_schema(Vault)['methods']) == ['total']


@pytest.mark.parametrize('name', ['__foo__', '__str__', '__private'])
def test_other_public_double_underscore_methods_are_still_rejected(name: str):
	# the bootloader forbids calling any ``__*`` method by name, so such a method
	# could never be called: that is a mistake worth reporting
	def method(self): ...

	method.__name__ = name
	Contract = type('Contract', (), {'__init__': lambda self: None, name: public(method)})

	with pytest.raises(TypeError, match='should not start with'):
		get_schema(Contract)


def test_real_contract_with_receive_and_fallback():
	import genlayer as gl
	from genlayer.contract import Contract

	class Vault(Contract):
		def __init__(self): ...

		@gl.public.view
		def total(self) -> int:
			return 0

		@gl.public.write.payable
		def __receive__(self):
			pass

		@gl.public.write
		def __handle_undefined_method__(self, method_name: str, args: list, kwargs: dict):
			pass

	import json

	schema = json.loads(Vault.__get_schema__())
	assert list(schema['methods']) == ['total']
