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


def public_name_error(name: str) -> str | None:
	"""
	Why a method called ``name`` cannot be public, if it cannot
	"""
	if name == '__init__':
		return (
			'the constructor `__init__` must not be public: remove its `gl.public` decorator'
		)
	if name in (
		'__receive__',
		'__handle_undefined_method__',
		'__handle_undefined_method_payable__',
	):
		return f'`{name}` is called by the VM only, so it must not be public: remove its `gl.public` decorator'
	if name.startswith('__'):
		return f'public method names must not start with `__`: `{name}`'
	return None
