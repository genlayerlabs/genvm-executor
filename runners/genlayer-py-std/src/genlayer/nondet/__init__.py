"""
Non-deterministic operations module.

This module provides APIs for operations that may produce different results
across different nodes, such as:
- ``exec_prompt``: Execute LLM prompts
- ``web``: Web interaction functionality
- ``Image``: Image dataclass for multimodal prompts
"""

__all__ = (
	'web',
	'exec_prompt',
	'Image',
	'JSONValue',
)

import collections.abc
import dataclasses
import json
import typing

import genlayer._internal.on_chain.gl_call as gl_call
from genlayer._internal import _lazy_api
from genlayer.types import Lazy


class NondetException(Exception):
	causes: list[str]
	ctx: dict[str, typing.Any]

	def __init__(self, causes: list[str], ctx: dict[str, typing.Any]):
		self.causes = causes
		self.ctx = ctx
		super().__init__(': '.join(causes))


import genlayer.calldata as calldata  # noqa: E402

type JSONValue = (
	None | bool | int | float | str | list[JSONValue] | dict[str, JSONValue]
)


def _invalid_nondet_response(reason: str) -> typing.NoReturn:
	raise NondetException(causes=[f'invalid nondeterministic response: {reason}'], ctx={})


def _decode_nondet(buf: collections.abc.Buffer) -> calldata.Decoded:
	try:
		ret = calldata.decode(buf)
	except (calldata.DecodingError, UnicodeDecodeError) as exc:
		_invalid_nondet_response(f'invalid calldata: {exc}')
	if not isinstance(ret, dict):
		_invalid_nondet_response('expected a dict')

	if 'error' in ret:
		err = ret['error']
		if isinstance(err, dict) and 'causes' in err:
			causes = err['causes']
			ctx = err.get('ctx', {})
			if (
				not isinstance(causes, list)
				or not all(isinstance(cause, str) for cause in causes)
				or not isinstance(ctx, dict)
			):
				_invalid_nondet_response('invalid error details')
			raise NondetException(causes=typing.cast(list[str], causes), ctx=ctx)
		raise NondetException(causes=[str(err)], ctx={})

	if 'ok' not in ret:
		_invalid_nondet_response('missing `ok` or `error`')
	return ret['ok']


def _decode_nondet_json(buf: collections.abc.Buffer) -> JSONValue:
	data = _decode_nondet(buf)
	if not isinstance(data, str | bytes | bytearray):
		_invalid_nondet_response('JSON result is not text')
	try:
		return typing.cast(JSONValue, json.loads(data))
	except (ValueError, UnicodeDecodeError, RecursionError) as exc:
		_invalid_nondet_response(f'invalid JSON: {exc}')


def _decode_nondet_text(buf: collections.abc.Buffer) -> str:
	data = _decode_nondet(buf)
	if not isinstance(data, str):
		_invalid_nondet_response('text result is not a string')
	return data


if typing.TYPE_CHECKING:
	import PIL.Image


@dataclasses.dataclass
class Image:
	raw: bytes
	pil: 'PIL.Image.Image'


class ExecPromptKwArgs(typing.TypedDict):
	response_format: typing.NotRequired[typing.Literal['text', 'json']]
	"""
	Defaults to ``text``
	"""
	images: typing.NotRequired[collections.abc.Sequence[bytes | Image] | None]


@typing.overload
def exec_prompt(
	prompt: str, *, images: collections.abc.Sequence[bytes | Image] | None = None
) -> str: ...


@typing.overload
def exec_prompt(
	prompt: str,
	*,
	response_format: typing.Literal['text'],
	images: collections.abc.Sequence[bytes | Image] | None = None,
) -> str: ...


@typing.overload
def exec_prompt(
	prompt: str,
	*,
	response_format: typing.Literal['json'],
	images: collections.abc.Sequence[bytes | Image] | None = None,
) -> JSONValue: ...


@_lazy_api
def exec_prompt(
	prompt: str, /, **config: typing.Unpack[ExecPromptKwArgs]
) -> Lazy[str | JSONValue]:
	"""
	API to execute a prompt (perform NLP)

	:param prompt: prompt itself
	:type prompt: ``str``

	:param \\*\\*config: configuration
	:type \\*\\*config: :py:class:`ExecPromptKwArgs`

	:rtype: ``str`` or :py:obj:`JSONValue`
	"""

	if len(prompt) == 0:
		raise ValueError('Prompt cannot be empty')

	images: list[bytes] = []
	for im in config.get('images', None) or []:
		if isinstance(im, Image):
			images.append(im.raw)
		elif isinstance(im, bytes):
			images.append(im)
		else:
			raise TypeError(f'expected bytes or Image, got {type(im).__name__}')

	format = config.get('response_format', 'text')

	data = {
		'ExecPrompt': {
			'prompt': prompt,
			'response_format': format,
			'images': images,
		}
	}
	if format == 'json':
		return gl_call.gl_call_generic(data, _decode_nondet_json)
	return gl_call.gl_call_generic(data, _decode_nondet_text)


import genlayer.nondet.web as web  # noqa: E402
