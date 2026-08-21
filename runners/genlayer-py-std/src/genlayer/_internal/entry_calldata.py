import typing

import genlayer._internal.reflect as reflect


def normalize(cd_raw: typing.Any) -> tuple[str, list, dict]:
	if not isinstance(cd_raw, dict):
		raise TypeError(
			f'invalid calldata, expected dict got `{reflect.repr_type(cd_raw)}`'
		)

	selector = cd_raw.get('', '')
	if not isinstance(selector, str):
		raise TypeError(
			'invalid calldata, expected method selector to be str, '
			f'got `{reflect.repr_type(selector)}`'
		)

	args = cd_raw.get('args', [])
	if not isinstance(args, list):
		raise TypeError(
			f'invalid calldata, expected `args` to be list, got `{reflect.repr_type(args)}`'
		)

	kwargs = cd_raw.get('kwargs', {})
	if not isinstance(kwargs, dict):
		raise TypeError(
			f'invalid calldata, expected `kwargs` to be dict, got `{reflect.repr_type(kwargs)}`'
		)
	if not all(isinstance(key, str) for key in kwargs):
		raise TypeError('invalid calldata, expected `kwargs` keys to be str')

	return selector, args, kwargs
