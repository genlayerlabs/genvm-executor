__all__ = (
	'render',
	'request',
	'get',
	'post',
	'put',
	'delete',
	'head',
	'options',
	'patch',
	'Response',
)

import collections.abc
import dataclasses
import io
import typing

import genlayer._internal.on_chain.gl_call as gl_call
from genlayer._internal import _lazy_api
from genlayer.types import Lazy

from . import Image, _decode_nondet, _invalid_nondet_response


@dataclasses.dataclass
class Response:
	status: int
	headers: dict[str, bytes]
	body: bytes | None


def str_or_bytes_to_bytes(
	data: str | bytes | None,
) -> bytes | None:
	if data is None:
		return None
	if isinstance(data, str):
		return data.encode('utf-8')
	return data


@_lazy_api
def get(
	url: str,
	/,
	*,
	headers: collections.abc.Mapping[str, str | bytes] | None = None,
	sign: bool = False,
) -> Lazy[Response]:
	return request.lazy(url, method='GET', headers=headers, sign=sign)


@_lazy_api
def post(
	url: str,
	/,
	*,
	body: str | bytes | None = None,
	headers: collections.abc.Mapping[str, str | bytes] | None = None,
	sign: bool = False,
) -> Lazy[Response]:
	return request.lazy(url, method='POST', body=body, headers=headers, sign=sign)


@_lazy_api
def put(
	url: str,
	/,
	*,
	body: str | bytes | None = None,
	headers: collections.abc.Mapping[str, str | bytes] | None = None,
	sign: bool = False,
) -> Lazy[Response]:
	return request.lazy(url, method='PUT', body=body, headers=headers, sign=sign)


@_lazy_api
def delete(
	url: str,
	/,
	*,
	body: str | bytes | None = None,
	headers: collections.abc.Mapping[str, str | bytes] | None = None,
	sign: bool = False,
) -> Lazy[Response]:
	return request.lazy(url, method='DELETE', body=body, headers=headers, sign=sign)


@_lazy_api
def head(
	url: str,
	/,
	*,
	body: str | bytes | None = None,
	headers: collections.abc.Mapping[str, str | bytes] | None = None,
	sign: bool = False,
) -> Lazy[Response]:
	return request.lazy(url, method='HEAD', body=body, headers=headers, sign=sign)


@_lazy_api
def options(
	url: str,
	/,
	*,
	body: str | bytes | None = None,
	headers: collections.abc.Mapping[str, str | bytes] | None = None,
	sign: bool = False,
) -> Lazy[Response]:
	return request.lazy(url, method='OPTIONS', body=body, headers=headers, sign=sign)


@_lazy_api
def patch(
	url: str,
	/,
	*,
	body: str | bytes | None = None,
	headers: collections.abc.Mapping[str, str | bytes] | None = None,
	sign: bool = False,
) -> Lazy[Response]:
	return request.lazy(url, method='PATCH', body=body, headers=headers, sign=sign)


@_lazy_api
def request(
	url: str,
	/,
	*,
	method: typing.Literal['GET', 'POST', 'PUT', 'DELETE', 'HEAD', 'OPTIONS', 'PATCH'],
	body: str | bytes | None = None,
	headers: collections.abc.Mapping[str, str | bytes] | None = None,
	sign: bool = False,
) -> Lazy[Response]:
	headers = headers or {}

	def decoder(data: collections.abc.Buffer) -> Response:
		result = _decode_nondet(data)
		if not isinstance(result, dict):
			_invalid_nondet_response('web result is not a dict')
		response = result.get('response')
		if not isinstance(response, dict):
			_invalid_nondet_response('missing web response')

		status = response.get('status')
		response_headers = response.get('headers')
		body = response.get('body')
		if (
			not isinstance(status, int)
			or isinstance(status, bool)
			or not 0 <= status <= 0xFFFF
		):
			_invalid_nondet_response('web response status is not a u16')
		if not isinstance(response_headers, dict) or not all(
			isinstance(key, str) and isinstance(value, bytes)
			for key, value in response_headers.items()
		):
			_invalid_nondet_response('web response headers are invalid')
		if body is not None and not isinstance(body, bytes):
			_invalid_nondet_response('web response body is invalid')
		return Response(
			status=status,
			headers=typing.cast(dict[str, bytes], response_headers),
			body=body,
		)

	return gl_call.gl_call_generic(
		{
			'WebRequest': {
				'url': url,
				'method': method,
				'body': str_or_bytes_to_bytes(body),
				'headers': {k: str_or_bytes_to_bytes(v) for k, v in headers.items()},
				'sign': sign,
			}
		},
		decoder,
	)


@typing.overload
def render(
	url: str,
	/,
	*,
	wait_after_loaded: str | None = None,
	mode: typing.Literal['text', 'html'] = 'text',
) -> str: ...


@typing.overload
def render(
	url: str,
	/,
	*,
	wait_after_loaded: str | None = None,
	mode: typing.Literal['screenshot'],
) -> Image: ...


@_lazy_api
def render(
	url: str,
	/,
	*,
	mode: typing.Literal['html', 'text', 'screenshot'] = 'text',
	wait_after_loaded: str | None = None,
) -> Lazy[str | Image]:
	"""
	API to get a webpage after rendering it in a browser-like environment

	:param url: url of website
	:param mode: Mode in which to return the result
	:param wait_after_loaded: How long to wait after dom loaded (for js to emit dynamic content). Should be in format such as "1000ms" or "1s"
	"""

	def decoder(x):
		x = _decode_nondet(x)
		if not isinstance(x, dict):
			_invalid_nondet_response('render result is not a dict')
		if mode != 'screenshot':
			text = x.get('text')
			if not isinstance(text, str):
				_invalid_nondet_response('render text is invalid')
			return text
		raw = x.get('image')
		if not isinstance(raw, bytes):
			_invalid_nondet_response('render image is invalid')
		import PIL.Image

		try:
			pil = PIL.Image.open(io.BytesIO(raw))
		except (OSError, ValueError) as exc:
			_invalid_nondet_response(f'render image is invalid: {exc}')
		return Image(raw, pil)

	return gl_call.gl_call_generic(
		{
			'WebRender': {
				'url': url,
				'mode': mode,
				'post_load_wait': wait_after_loaded or '0ms',
			}
		},
		decoder,
	)
