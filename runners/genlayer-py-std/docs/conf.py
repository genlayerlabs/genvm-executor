"""
Sphinx settings for the reference pages of the runners built from this directory
"""

import enum
import os
import sys
import typing
from pathlib import Path
from types import ModuleType

_runner_dir = Path(__file__).resolve().parents[1]

_ignored_special = [
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


def configure(ns: dict[str, typing.Any]) -> None:
	sys.path.insert(0, str(_runner_dir / 'src'))
	sys.path.insert(0, str(_runner_dir / 'src-emb'))

	os.environ['GENERATING_DOCS'] = 'true'

	fake_genlayer_wasi = ModuleType('_genlayer_wasi')
	fake_genlayer_wasi.__dict__['FAKE_VM'] = True
	sys.modules['_genlayer_wasi'] = fake_genlayer_wasi

	ns['extensions'] = [*ns['extensions'], 'sphinx.ext.autodoc', 'sphinx.ext.todo']
	ns['intersphinx_mapping'] = {
		**ns['intersphinx_mapping'],
		'python': ('https://docs.python.org/3.12', None),
		'numpy': ('https://numpy.org/doc/stable/', None),
	}
	ns['todo_include_todos'] = True
	ns['autodoc_mock_imports'] = [
		'_genlayer_wasi',
		'google',
		'onnx',
		'word_piece_tokenizer',
	]
	ns['autodoc_default_options'] = {
		'inherited-members': True,
		'private-members': False,
		'special-members': True,
		'exclude-members': ','.join(_ignored_special + ['gl']),
	}
	ns['autodoc_class_signature'] = 'separated'
	ns['autodoc_typehints'] = 'both'
	ns['autodoc_typehints_description_target'] = 'documented_params'
	ns['autodoc_inherit_docstrings'] = True
	ns['autodoc_typehints_format'] = 'short'
	ns['autodoc_preserve_defaults'] = True
	ns['autodoc_signature_line_length'] = 1
	ns['setup'] = _setup


def _setup(app):
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

	# u8..u256, i8..i256 display aliases for Annotated[int, StaticIntMeta(...)]
	type_display_aliases = {}
	for prefix, signed in [('u', False), ('i', True)]:
		for size in range(1, 33):
			bits = size * 8
			for annotated in ('typing.Annotated', 'Annotated'):
				type_display_aliases[
					f'{annotated}[int, StaticIntMeta(size={size}, signed={signed})]'
				] = f'genlayer.types.{prefix}{bits}'

	def autodoc_process_signature(
		app, what, name, obj, options, signature, return_annotation
	):
		for old, new in type_display_aliases.items():
			if signature:
				signature = signature.replace(old, new)
			if return_annotation:
				return_annotation = return_annotation.replace(old, new)
		return (signature, return_annotation)

	app.connect('autodoc-process-bases', handle_bases)
	app.connect('autodoc-skip-member', handle_skip_member)
	app.connect('autodoc-process-signature', autodoc_process_signature)
