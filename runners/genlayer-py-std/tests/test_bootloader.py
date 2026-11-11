import runpy
import sys
import types

import genlayer as gl
import genlayer.contract as contract
import pytest
from genlayer._internal import entry_calldata


@pytest.fixture
def dispatch(monkeypatch):
	monkeypatch.setenv('GENERATING_DOCS', 'true')
	monkeypatch.setattr(sys, 'path', sys.path.copy())
	monkeypatch.setitem(
		sys.modules,
		'genlayer._internal.on_chain.storage',
		types.ModuleType('genlayer._internal.on_chain.storage'),
	)
	monkeypatch.setitem(sys.modules, 'contract', types.ModuleType('contract'))
	main = runpy.run_module('_genlayer_bootloader')['_handle_main']

	def run(klass, calldata, value, *, is_init=False):
		monkeypatch.setattr(contract, '__known_contract__', klass)
		monkeypatch.setattr(
			gl.message,
			'raw',
			{'entry_data': gl.calldata.encode(calldata), 'is_init': is_init, 'value': value},
		)
		monkeypatch.setattr(
			gl.storage.Root,
			'get',
			lambda: types.SimpleNamespace(
				get_contract_instance=lambda cls: cls(), lock_default=lambda: None
			),
		)
		main()

	return run


@pytest.mark.parametrize('value', [0, 100])
@pytest.mark.parametrize(
	'calldata', [{}, {'': ''}, {'': '', 'args': [1], 'kwargs': {'x': 2}}]
)
@pytest.mark.parametrize('has_handlers', [False, True])
def test_empty_method_is_rejected_without_dispatch(
	dispatch, value, calldata, has_handlers
):
	class Methods:
		__handle_undefined_method__ = contract.Contract.__handle_undefined_method__
		__handle_undefined_method_payable__ = (
			contract.Contract.__handle_undefined_method_payable__
		)

	if has_handlers:

		def unexpected(*args, **kwargs):
			pytest.fail('empty method must not invoke either fallback')

		Methods.__handle_undefined_method__ = unexpected
		Methods.__handle_undefined_method_payable__ = unexpected

	with pytest.raises(ValueError, match='^empty method name$'):
		dispatch(Methods, calldata, value)


def test_contract_has_no_receive_hook():
	assert not hasattr(contract.Contract, '__receive__')


@pytest.mark.parametrize(
	'decorate', [gl.public.view, gl.public.write, gl.public.write.payable]
)
def test_receive_name_reports_removal_even_when_decorated(decorate):
	def __receive__(self): ...

	with pytest.raises(
		TypeError,
		match=r'`__receive__` was removed in v0\.4; contracts have no receive hook',
	):
		decorate(__receive__)


@pytest.mark.parametrize(
	'entry, is_init', [({}, True), ({'': 'run'}, False), ({'': '#get-schema'}, False)]
)
def test_runtime_rejects_contract_defining_receive(
	dispatch, monkeypatch, tmp_path, entry, is_init
):
	(tmp_path / 'contract.py').write_text("""import genlayer as gl

class Invalid(gl.contract.Contract):
	def __init__(self):
		raise AssertionError('invalid contract must not be instantiated')

	def __receive__(self):
		raise AssertionError('removed hook must not run')

	@gl.public.write
	def run(self):
		raise AssertionError('invalid contract must not run')
""")
	monkeypatch.delitem(sys.modules, 'contract')
	monkeypatch.syspath_prepend(str(tmp_path))
	from genlayer._internal.on_chain import gl_call

	def unexpected_result(result):
		pytest.fail('invalid contract must not return a schema')

	monkeypatch.setattr(gl_call, 'contract_return', unexpected_result)
	with pytest.raises(
		TypeError,
		match=r'`__receive__` was removed in v0\.4; contracts have no receive hook',
	):
		dispatch(None, entry, 0, is_init=is_init)
	assert contract.__known_contract__ is None


@pytest.mark.parametrize('value', [0, 100])
def test_nonempty_missing_method_still_reaches_fallback(dispatch, value):
	class Called(Exception):
		pass

	class Methods:
		def __handle_undefined_method__(self, method, args, kwargs):
			assert value == 0
			assert (method, args, kwargs) == ('missing', [1], {'x': 2})
			raise Called()

		def __handle_undefined_method_payable__(self, method, args, kwargs):
			assert value == 100
			assert (method, args, kwargs) == ('missing', [1], {'x': 2})
			raise Called()

	with pytest.raises(Called):
		dispatch(Methods, {'': 'missing', 'args': [1], 'kwargs': {'x': 2}}, value)


def test_normalize_calldata():
	assert entry_calldata.normalize({}) == ('', [], {})
	assert entry_calldata.normalize(
		{'': 'method', 'args': [1], 'kwargs': {'key': 2}}
	) == ('method', [1], {'key': 2})


@pytest.mark.parametrize(
	'calldata',
	[
		[],
		{'': 1},
		{'args': ()},
		{'kwargs': []},
		{'kwargs': {1: 'value'}},
	],
)
def test_normalize_calldata_rejects_malformed_fields(calldata):
	with pytest.raises(TypeError, match='invalid calldata'):
		entry_calldata.normalize(calldata)
