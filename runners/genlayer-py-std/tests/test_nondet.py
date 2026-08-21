import json

import genlayer.calldata as calldata
import genlayer.nondet as nondet
import genlayer.nondet.web as web
import pytest
from genlayer.types import Lazy


def _encoded(value) -> bytes:
	return calldata.encode(value)


@pytest.mark.parametrize(
	'value',
	[None, True, 3, 1.5, 'text', [1, 'two'], {'nested': [False]}],
)
def test_decode_nondet_json_accepts_every_json_value(value):
	assert nondet._decode_nondet_json(_encoded({'ok': json.dumps(value)})) == value


@pytest.mark.parametrize(
	'response',
	[[], {}, {'error': {'causes': 'not a list'}}, {'error': {'causes': [], 'ctx': []}}],
)
def test_decode_nondet_rejects_malformed_envelopes(response):
	with pytest.raises(nondet.NondetException, match='invalid nondeterministic response'):
		nondet._decode_nondet(_encoded(response))


@pytest.mark.parametrize('value', ['{', '1' * 5000])
def test_decode_nondet_json_wraps_malformed_result(value):
	with pytest.raises(nondet.NondetException, match='invalid JSON'):
		nondet._decode_nondet_json(_encoded({'ok': value}))


def test_decode_nondet_wraps_malformed_calldata():
	with pytest.raises(nondet.NondetException, match='invalid calldata'):
		nondet._decode_nondet(b'')


def test_exec_prompt_rejects_non_image_sequence_elements(monkeypatch):
	with pytest.raises(TypeError, match='expected bytes or Image'):
		nondet.exec_prompt('prompt', images=b'not-a-sequence-of-images')


def test_web_exports_put_and_options():
	assert 'put' in web.__all__
	assert 'options' in web.__all__


@pytest.mark.parametrize(
	('helper', 'method'), [(web.put, 'PUT'), (web.options, 'OPTIONS')]
)
def test_web_method_helpers(monkeypatch, helper, method):
	requests = []

	def call(request, decoder):
		requests.append(request)
		return Lazy(
			lambda: decoder(
				_encoded({'ok': {'response': {'status': 200, 'headers': {}, 'body': b''}}})
			)
		)

	monkeypatch.setattr(web.gl_call, 'gl_call_generic', call)
	assert helper('https://example.com').status == 200
	assert requests[0]['WebRequest']['method'] == method


def test_render_default_mode(monkeypatch):
	def call(request, decoder):
		assert request['WebRender']['mode'] == 'text'
		return Lazy(lambda: decoder(_encoded({'ok': {'text': 'rendered'}})))

	monkeypatch.setattr(web.gl_call, 'gl_call_generic', call)
	assert web.render('https://example.com') == 'rendered'


def test_render_rejects_malformed_image(monkeypatch):
	monkeypatch.setattr(
		web.gl_call,
		'gl_call_generic',
		lambda _request, decoder: Lazy(
			lambda: decoder(_encoded({'ok': {'image': b'not-an-image'}}))
		),
	)
	with pytest.raises(nondet.NondetException, match='render image is invalid'):
		web.render('https://example.com', mode='screenshot')


@pytest.mark.parametrize(
	'response',
	[
		{'ok': []},
		{'ok': {}},
		{'ok': {'response': {'status': '200', 'headers': {}, 'body': b''}}},
		{'ok': {'response': {'status': -1, 'headers': {}, 'body': b''}}},
		{'ok': {'response': {'status': 65536, 'headers': {}, 'body': b''}}},
		{'ok': {'response': {'status': 200, 'headers': {'x': 'text'}, 'body': b''}}},
		{'ok': {'response': {'status': 200, 'headers': {}, 'body': 'text'}}},
	],
)
def test_web_request_rejects_malformed_responses(monkeypatch, response):
	monkeypatch.setattr(
		web.gl_call,
		'gl_call_generic',
		lambda _request, decoder: Lazy(lambda: decoder(_encoded(response))),
	)
	with pytest.raises(nondet.NondetException, match='invalid nondeterministic response'):
		web.get('https://example.com')
