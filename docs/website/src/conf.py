import datetime
import enum
import json
import os
import sys
import typing
from pathlib import Path
from types import ModuleType

# executors/<line>.x/ — three parents up from docs/website/src/conf.py
_root = Path(__file__).resolve().parents[3]
_manifest = json.loads((_root / 'manifest.json').read_text())

project = 'GenVM Executor'
author = 'GenLayer Labs'

copyright_year = os.environ.get('COPYRIGHT_YEAR', str(datetime.date.today().year))
copyright = f'{copyright_year}, GenLayer Labs'

# This sub-site's version is always the line's own executor-version. Do NOT
# read DOCS_VERSION here: the deploy sets it to the *manager* version, which
# would otherwise leak in and make every line's sub-site claim the same version.
release = _manifest['executor-version']
version = release

extensions = [
	'sphinx.ext.autodoc',
	'sphinx.ext.viewcode',
	'sphinx.ext.todo',
	'sphinx.ext.intersphinx',
	'sphinxcontrib.mermaid',
	'myst_parser',
]

exclude_patterns = [
	'_build',
	'Thumbs.db',
	'.DS_Store',
	# `.. include::`d into changelog.rst; excluding them from the doc set only
	# suppresses the "not in any toctree" orphan warnings (include still works).
	'python-sdk/changelog-notes/*',
]

language = 'en'

mermaid_version = '11.6.0'
mermaid_output_format = 'svg'
mermaid_params = ['--theme', 'dark', '--backgroundColor', 'transparent']

html_theme = 'pydata_sphinx_theme'

master_doc = 'index'

# --- Python SDK autodoc -----------------------------------------------------
# The SDK lives in THIS line's runner tree; autodoc imports it from there.
sys.path.insert(0, str(_root / 'runners' / 'genlayer-py-std' / 'src'))
sys.path.insert(0, str(_root / 'runners' / 'genlayer-py-std' / 'src-emb'))

os.environ['GENERATING_DOCS'] = 'true'

todo_include_todos = True

autodoc_mock_imports = ['_genlayer_wasi', 'google', 'onnx', 'word_piece_tokenizer']

fake_genlayer_wasi = ModuleType('_genlayer_wasi')
fake_genlayer_wasi.__dict__['FAKE_VM'] = True
sys.modules['_genlayer_wasi'] = fake_genlayer_wasi

# --- intersphinx ------------------------------------------------------------
# Cross-links back to the manager site (spec / impl-spec pages the SDK runners
# page points at). The manager builds first (see the manager's docs.py), so its
# objects.inv is on disk by the time this sub-site builds. The base URL is where
# the manager site is served: https://<domain>/<manager-version>/ .
_docs_domain = os.environ.get('DOCS_DOMAIN', 'sdk.genlayer.com')
_manager_version = os.environ.get('DOCS_VERSION', 'main')

_manager_root = _root
while not (_manager_root / '.genvm-monorepo-root').exists():
	_manager_root = _manager_root.parent
_manager_inv = _manager_root / 'build' / 'doc' / 'html' / 'objects.inv'

intersphinx_mapping = {
	'python': ('https://docs.python.org/3.12', None),
	'numpy': ('https://numpy.org/doc/stable/', None),
	'genvm': (
		f'https://{_docs_domain}/{_manager_version}/',
		str(_manager_inv) if _manager_inv.exists() else None,
	),
}

# --- autodoc rendering ------------------------------------------------------
ignored_special = [
	'__dict__',
	'__abstractmethods__',
	'__annotations__',
	'__class_getitem__',
	'__init_subclass__',
	'__module__',
	'__orig_bases__',
	'__parameters__',
	'__slots__',
	'__subclasshook__',
	'__type_params__',
	'__weakref__',
	'__reversed__',
	'__protocol_attrs__',
	'__dataclass_fields__',
	'__match_args__',
	'__dataclass_params__',
]

autodoc_default_options: dict[str, str | bool] = {
	'inherited-members': True,
	'private-members': False,
	'special-members': True,
	'exclude-members': ','.join(ignored_special + ['gl']),
}

autoapi_python_class_content = 'class'
autodoc_class_signature = 'separated'
autodoc_typehints = 'both'
autodoc_typehints_description_target = 'documented_params'
autodoc_inherit_docstrings = True
autodoc_typehints_format = 'short'
autodoc_preserve_defaults = True
autodoc_signature_line_length = 1


def setup(app):
	def handle_bases(app, name, obj, options, bases: list):
		idx = 0
		for i in range(len(bases)):
			cur = bases[i]
			cur_name = cur if isinstance(cur, str) else cur.__qualname__
			if cur_name.startswith('_'):
				pass
			else:
				bases[idx] = cur
				idx += 1
		bases[idx:] = []
		if len(bases) == 0:
			bases.append(object)

	def handle_skip_member(app, what, name, obj, skip, options):
		import types

		if 'what' == 'class' and isinstance(obj, types.MethodType):
			if obj.__self__.__class__ is typing.NewType:
				return True
		# Sphinx 9's autodoc options object is attribute-based (underscored) and
		# forbids the old item assignment; None is the "off" value for
		# inherited_members.
		if what == 'module' and isinstance(obj, type):
			if any(base in obj.mro() for base in [dict, tuple, bytes, enum.Enum]):
				options.special_members = []
				options.inherited_members = None
				return
		if what == 'module':
			if isinstance(obj, typing.NewType):
				options.special_members = []
				options.inherited_members = None
				return

	# Build u8..u256, i8..i256 display aliases for Annotated[int, StaticIntMeta(...)]
	_type_display_aliases = {}
	for _prefix, _signed in [('u', False), ('i', True)]:
		for _sz in range(1, 33):
			_bits = _sz * 8
			_type_display_aliases[
				f'typing.Annotated[int, StaticIntMeta(size={_sz}, signed={_signed})]'
			] = f'genlayer.types.{_prefix}{_bits}'
			_type_display_aliases[
				f'Annotated[int, StaticIntMeta(size={_sz}, signed={_signed})]'
			] = f'genlayer.types.{_prefix}{_bits}'

	def autodoc_process_signature(
		app, what, name, obj, options, signature, return_annotation
	):
		for old, new in _type_display_aliases.items():
			if signature:
				signature = signature.replace(old, new)
			if return_annotation:
				return_annotation = return_annotation.replace(old, new)
		return (signature, return_annotation)

	app.connect('autodoc-process-bases', handle_bases)
	app.connect('autodoc-skip-member', handle_skip_member)
	app.connect('autodoc-process-signature', autodoc_process_signature)
