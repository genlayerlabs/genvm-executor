"""
Attribute names used by decorators and schema generation.

This module exists as a leaf dependency so both genlayer._internal.annotations
and genlayer._internal.get_schema can import these constants without circular imports.
"""

PUBLIC_ATTR = '__gl_public__'
READONLY_ATTR = '__gl_readonly__'
MIN_GAS_LEADER_ATTR = '__gl_min_gas_leader__'
MIN_GAS_VALIDATOR_ATTR = '__gl_min_gas_validator__'
PAYABLE_ATTR = '__gl_payable__'

_REMOVED_RECEIVE_ERROR = (
	'`__receive__` was removed in v0.4; contracts have no receive hook'
)


def reject_removed_receive_hook(contract: type) -> None:
	if any('__receive__' in vars(base) for base in contract.__mro__):
		raise TypeError(_REMOVED_RECEIVE_ERROR)


def public_name_error(name: str) -> str | None:
	"""
	Error message if a method called ``name`` cannot be public
	"""
	if name == '__receive__':
		return _REMOVED_RECEIVE_ERROR
	if name == '__init__':
		return (
			'the constructor `__init__` must not be public: remove its `gl.public` decorator'
		)
	if name in (
		'__handle_undefined_method__',
		'__handle_undefined_method_payable__',
	):
		return f'`{name}` is called by the VM only, so it must not be public: remove its `gl.public` decorator'
	if name.startswith('__'):
		return f'public method names must not start with `__`: `{name}`'
	return None
