"""
Generate Python views over GenVM's slot-based storage format.
"""

# ruff: noqa: F403, F405

__all__ = ('generate_storage',)

import datetime
import struct
import sys
import time
import typing

import genlayer._internal.reflect as reflect
from genlayer.storage.core import *
from genlayer.types import *  # noqa: F403
from genlayer.types import StaticIntMeta

from ..array import Array, _ArrayDesc
from ..dyn_array import DynArray, _DynArrayDesc
from .desc_base_types import (
	AddrDesc,
	BoolDesc,
	BytesDesc,
	IntDesc,
	NoneDesc,
	StrDesc,
	_BigIntDesc,
)
from .desc_record import RecordExtraFields, RecordField, RecordLayout, _RecordDesc

STORAGE_PATCHED_ATTR = '__gl_storage_patched__'
ORIGINAL_INIT_ATTR = '__gl_original_init__'
ALLOW_STORAGE_ATTR = '__gl_allow_storage__'

_MAX_STORAGE_SIZE = 2**32 - 1
_MISSING = object()


def allow[T: type](cls: T) -> T:
	"""
	Marks class as allowed to be used within storage.
	Without this annotation, storage builder will raise an exception
	when trying to generate description for the class.
	This behavior is required to prevent accidental usage of classes that are not designed to be used in storage,
	because storage-generated class is modified and starts to behave differently from regular python class)
	"""
	setattr(cls, ALLOW_STORAGE_ATTR, True)
	return cls


def generate_storage[T: type](cls: T) -> T:
	populate_np_descs_if_loaded()
	allow(cls)
	desc = _storage_build(_BuilderCtx.empty(), cls)
	_known_descs[cls] = desc
	return cls


_none_desc = NoneDesc()
_bigint_desc = _BigIntDesc()

_all_int_types: tuple = (
	u8,
	u16,
	u24,
	u32,
	u40,
	u48,
	u56,
	u64,
	u72,
	u80,
	u88,
	u96,
	u104,
	u112,
	u120,
	u128,
	u136,
	u144,
	u152,
	u160,
	u168,
	u176,
	u184,
	u192,
	u200,
	u208,
	u216,
	u224,
	u232,
	u240,
	u248,
	u256,
	i8,
	i16,
	i24,
	i32,
	i40,
	i48,
	i56,
	i64,
	i72,
	i80,
	i88,
	i96,
	i104,
	i112,
	i120,
	i128,
	i136,
	i144,
	i152,
	i160,
	i168,
	i176,
	i184,
	i192,
	i200,
	i208,
	i216,
	i224,
	i232,
	i240,
	i248,
	i256,
)

_int_descs: dict[StaticIntMeta, tuple[type, IntDesc]] = {}
for _int_type in _all_int_types:
	_int_meta: StaticIntMeta = _int_type.__metadata__[0]
	_int_descs[_int_meta] = (
		_int_type,
		IntDesc(_int_meta.size, signed=_int_meta.signed),
	)

_known_descs: dict[typing.Any, TypeDesc] = {
	Address: AddrDesc(),
	str: StrDesc(),
	bytes: BytesDesc(),
	bool: BoolDesc(),
	type(None): _none_desc,
	None: _none_desc,  # type: ignore
	bigint: _bigint_desc,
}

for _int_type, _int_desc in _int_descs.values():
	_known_descs[_int_type] = _int_desc


class _FloatDesc(TypeDesc[float]):
	__slots__ = ()

	def __init__(self):
		TypeDesc.__init__(self, 8, [8])

	def get(self, slot: Slot, off: int) -> float:
		return struct.unpack('d', slot.read(off, self.size))[0]

	def set(self, slot: Slot, off: int, val: float) -> None:
		slot.write(off, struct.pack('d', val))


_known_descs[float] = _FloatDesc()

_layout_cache: dict[typing.Any, TypeDesc] = {}
_field_cache: dict[type, tuple[tuple[str, typing.Any], ...]] = {}


class GenerationError(TypeError):
	pass


class _BuilderCtx(typing.NamedTuple):
	generic_vars: dict[typing.TypeVar, typing.Any]
	trace: tuple[str, ...]
	active: tuple[typing.Any, ...] = ()

	def with_trace(self, msg: str) -> '_BuilderCtx':
		return self._replace(trace=self.trace + (msg,))

	def type_err(self, msg: str) -> GenerationError:
		exc = GenerationError(msg)
		for trace in self.trace:
			exc.add_note(trace)
		return exc

	@staticmethod
	def empty() -> '_BuilderCtx':
		return _BuilderCtx({}, ())


def _resolve_raw_type(ctx: _BuilderCtx, typ: typing.Any) -> typing.Any:
	if isinstance(typ, typing.TypeVar):
		resolved = ctx.generic_vars.get(typ, _MISSING)
		if resolved is _MISSING:
			raise ctx.type_err(f'Unbound generic type variable `{typ.__name__}`')
		if resolved is typ:
			return typ
		return _resolve_raw_type(ctx, resolved)

	origin = typing.get_origin(typ)
	if origin is None or origin is typing.Literal:
		return typ

	args = typing.get_args(typ)
	if origin is typing.Annotated:
		base = _resolve_raw_type(ctx, args[0])
		if base is args[0]:
			return typ
		return typing.Annotated[base, *args[1:]]

	resolved_args = tuple(_resolve_raw_type(ctx, arg) for arg in args)
	if resolved_args == args:
		return typ

	copy_with = getattr(typ, 'copy_with', None)
	if copy_with is not None:
		return copy_with(resolved_args)

	try:
		if len(resolved_args) == 1:
			return origin[resolved_args[0]]
		return origin[resolved_args]
	except (TypeError, AttributeError) as exc:
		err = ctx.type_err(f'Unable to resolve generic type `{reflect.repr_type(typ)}`')
		raise err from exc


def _cache_get(cache: dict, key: typing.Any) -> typing.Any:
	try:
		return cache.get(key, _MISSING)
	except TypeError:
		return _MISSING


def _cache_set(cache: dict, key: typing.Any, value: typing.Any) -> None:
	try:
		cache[key] = value
	except TypeError:
		pass


def _storage_build(ctx: _BuilderCtx, cls: typing.Any) -> TypeDesc:
	cls = _resolve_raw_type(ctx, cls)

	known = _cache_get(_known_descs, cls)
	if known is not _MISSING:
		return known
	known = _cache_get(_layout_cache, cls)
	if known is not _MISSING:
		return known

	if cls in ctx.active:
		raise ctx.type_err(
			f'Recursive storage type `{reflect.repr_type(cls)}` requires an unsupported incomplete layout'
		)

	trace_note = ['during building type `', reflect.repr_type(cls), '`']
	if lineno := reflect.try_get_lineno(cls):
		trace_note.extend((' (declared at ', str(lineno), ')'))
	ctx = ctx.with_trace(''.join(trace_note))._replace(active=ctx.active + (cls,))

	desc = _storage_build_impl(ctx, cls)
	_cache_set(_layout_cache, cls, desc)
	return desc


def _check_forbidden_origin(ctx: _BuilderCtx, cls: typing.Any) -> None:
	if cls is int:
		raise ctx.type_err('use `bigint` or one of sized integers')
	if cls is dict:
		raise ctx.type_err('use `TreeMap` instead of a dict')
	if cls is list:
		raise ctx.type_err('use `DynArray` instead of a list')


def _generic_bindings(
	ctx: _BuilderCtx,
	origin: type,
	generic_params: tuple[typing.TypeVar, ...],
	args: tuple[typing.Any, ...],
) -> dict[typing.TypeVar, typing.Any]:
	bindings = dict(zip(generic_params, args))
	pending = [origin]
	seen: set[type] = set()
	while pending:
		current = pending.pop()
		if current in seen:
			continue
		seen.add(current)
		current_ctx = ctx._replace(generic_vars={**ctx.generic_vars, **bindings})
		raw_bases = list(current.__dict__.get('__orig_bases__', ()))
		parameterized_bases = {typing.get_origin(base) or base for base in raw_bases}
		raw_bases.extend(
			base for base in current.__bases__ if base not in parameterized_bases
		)
		for raw_base in raw_bases:
			base = _resolve_raw_type(current_ctx, raw_base)
			base_origin = typing.get_origin(base) or base
			if base_origin is typing.Generic or base_origin is object:
				continue
			base_params = getattr(base_origin, '__type_params__', ())
			base_args = typing.get_args(base)
			if base_params and len(base_params) != len(base_args):
				continue
			bindings.update(zip(base_params, base_args))
			pending.append(base_origin)
	return bindings


def _storage_build_impl(ctx: _BuilderCtx, cls: typing.Any) -> TypeDesc:
	origin = typing.get_origin(cls)
	if origin is None:
		_check_forbidden_origin(ctx, cls)
		bindings = _generic_bindings(ctx, cls, (), ())
		return _storage_build_struct(ctx._replace(generic_vars=bindings), cls)

	args = typing.get_args(cls)
	if 'numpy' in sys.modules and origin is sys.modules['numpy'].dtype:
		if len(args) != 1:
			raise ctx.type_err(
				f'Expected exactly one argument for numpy dtype, got {len(args)}'
			)
		return _storage_build(ctx, args[0])

	_check_forbidden_origin(ctx, origin)

	if origin is typing.Annotated:
		return _storage_build_annotated(ctx, cls)
	if origin is typing.Literal:
		raise ctx.type_err('Literal types are not supported in storage')
	if origin is tuple or origin is typing.Tuple:
		raise ctx.type_err(
			'Tuple types are not supported in storage, use a custom class instead'
		)

	if (numpy_desc := try_handle_np(ctx, origin, args)) is not None:
		return numpy_desc

	generic_params = getattr(origin, '__type_params__', ())
	if len(generic_params) != len(args):
		raise ctx.type_err(
			f'incorrect number of generic arguments for {origin} parameters={generic_params}, args={args}'
		)

	if origin is Array:
		return _storage_build_array(ctx, cls)
	if origin is DynArray:
		elem_desc = _storage_build(
			ctx.with_trace('during processing DynArray element type'), args[0]
		)
		return _DynArrayDesc(elem_desc)
	if origin is Indirection:
		elem_desc = _storage_build(
			ctx.with_trace('during processing Indirection element type'), args[0]
		)
		return IndirectionTypeDesc(elem_desc)
	if origin is VLA:
		elem_desc = _storage_build(
			ctx.with_trace('during processing VLA element type'), args[0]
		)
		return VLATypeDesc(elem_desc)

	bindings = _generic_bindings(ctx, origin, generic_params, args)
	declared = ', '.join(param.__name__ for param in generic_params)
	return _storage_build_struct(
		ctx._replace(generic_vars=bindings).with_trace(
			f'declared generic variables: {declared}'
		),
		origin,
	)


def _storage_build_array(ctx: _BuilderCtx, cls: typing.Any) -> TypeDesc:
	type_arg, size_arg = typing.get_args(cls)
	if typing.get_origin(size_arg) is not typing.Literal:
		raise ctx.type_err(f'Expected Literal for Array size, got {size_arg}')
	lit_args = typing.get_args(size_arg)
	if len(lit_args) != 1 or type(lit_args[0]) is not int:
		raise ctx.type_err(f'Expected single int Literal for Array size, got {lit_args}')
	size = lit_args[0]
	if size <= 0:
		raise ctx.type_err(f'Array size must be strictly positive, got {size}')
	item_desc = _storage_build(
		ctx.with_trace('during processing Array element type'), type_arg
	)
	if item_desc.size != 0 and size > _MAX_STORAGE_SIZE // item_desc.size:
		raise ctx.type_err('Array storage size exceeds the 32-bit storage address space')
	return _ArrayDesc(item_desc, size)


def _storage_build_annotated(ctx: _BuilderCtx, cls: typing.Any) -> TypeDesc:
	origin = getattr(cls, '__origin__', None)
	if origin is None:
		raise ctx.type_err('typing.Annotated should have __origin__')

	if origin is int:
		for metadata in getattr(cls, '__metadata__', ()):
			if metadata == 'bigint':
				return _bigint_desc
			if isinstance(metadata, StaticIntMeta):
				return _int_descs[metadata][1]

	return _storage_build(ctx.with_trace('during processing discarded annotated'), origin)


def _storage_fields(cls: type) -> tuple[tuple[str, typing.Any], ...]:
	cached = _field_cache.get(cls)
	if cached is not None:
		return cached
	fields = tuple(typing.get_type_hints(cls, include_extras=True).items())
	_field_cache[cls] = fields
	return fields


def _iter_storage_type_vars(typ: typing.Any) -> typing.Iterator[typing.TypeVar]:
	if isinstance(typ, typing.TypeVar):
		yield typ
		return
	args = typing.get_args(typ)
	if typing.get_origin(typ) is typing.Annotated:
		args = args[:1]
	for arg in args:
		yield from _iter_storage_type_vars(arg)


def _find_layout_parameter(
	typ: typing.Any,
	symbolic_ctx: _BuilderCtx,
	parameters: frozenset[typing.TypeVar],
) -> typing.TypeVar | None:
	for type_var in _iter_storage_type_vars(typ):
		resolved = _resolve_raw_type(symbolic_ctx, type_var)
		for dependency in _iter_storage_type_vars(resolved):
			if dependency in parameters:
				return dependency
	return None


def _install_storage_view(
	cls: type,
	description: _RecordDesc,
	field_names: tuple[str, ...],
	generic_usage: tuple[typing.TypeVar, _BuilderCtx] | None,
) -> None:
	if cls.__dict__.get(STORAGE_PATCHED_ATTR, False):
		return

	for field_name in field_names:

		def getter(s: RecordExtraFields, name: str = field_name):
			field = s.__type_desc__.layout.fields[name]
			return field.desc.get(s._storage_slot, s._off + field.offset)

		def setter(s: RecordExtraFields, value, name: str = field_name) -> None:
			field = s.__type_desc__.layout.fields[name]
			field.desc.set(s._storage_slot, s._off + field.offset, value)

		setattr(
			cls,
			field_name,
			property(
				getter,
				setter,
				doc='Storage-backed field; failed compound assignment can leave it partially updated.',
			),
		)

	setattr(cls, STORAGE_PATCHED_ATTR, True)
	old_init = cls.__init__
	if hasattr(cls, '__gl_contract__'):
		return
	if getattr(old_init, STORAGE_PATCHED_ATTR, False):
		old_init = getattr(old_init, ORIGINAL_INIT_ATTR)

	if generic_usage is not None:
		generic_var, usage_ctx = generic_usage

		def new_init(self, *args, **kwargs):
			if hasattr(self, '_storage_slot'):
				old_init(self, *args, **kwargs)
				return
			exc = usage_ctx.type_err(
				'generic storage classes can not be instantiated with __init__, '
				'please, use gl.storage.inmem_allocate'
			)
			exc.add_note(f'due to usage of `{generic_var.__name__}`')
			exc.add_note(f'in class `{reflect.repr_type(cls)}`')
			raise exc

	else:

		def new_init(self, *args, **kwargs):
			if not hasattr(self, '_storage_slot'):
				self._storage_slot = InmemManager().get_store_slot(ROOT_SLOT_ID)
				self._off = 0
				self.__type_desc__ = description
			old_init(self, *args, **kwargs)

	setattr(new_init, STORAGE_PATCHED_ATTR, True)
	setattr(new_init, ORIGINAL_INIT_ATTR, old_init)
	cls.__init__ = new_init


def _storage_build_struct(ctx: _BuilderCtx, cls: type) -> TypeDesc:
	if not hasattr(cls, ALLOW_STORAGE_ATTR):
		raise ctx.type_err(
			'class is not marked for usage within storage, please annotate it with @allow'
		)

	size = 0
	copy_actions: list[CopyAction] = []
	fields: dict[str, RecordField] = {}
	generic_usage: tuple[typing.TypeVar, _BuilderCtx] | None = None
	generic_params = tuple(getattr(cls, '__type_params__', ()))
	parameter_set = frozenset(generic_params)
	symbolic_ctx = _BuilderCtx(
		_generic_bindings(_BuilderCtx.empty(), cls, generic_params, generic_params),
		(),
	)

	for field_name, field_type in _storage_fields(cls):
		if typing.get_origin(field_type) is typing.ClassVar:
			continue

		note = f'during processing field `{field_name}: {field_type}`'
		if parameter_set and generic_usage is None:
			if generic_var := _find_layout_parameter(field_type, symbolic_ctx, parameter_set):
				usage_ctx = ctx.with_trace(note).with_trace(
					f'during building type `{reflect.repr_type(generic_var)}`'
				)
				generic_usage = (generic_var, usage_ctx)
		try:
			field_desc = _storage_build(ctx.with_trace(note), field_type)
		except GenerationError:
			raise
		except BaseException as exc:
			exc.add_note(note)
			raise

		if field_desc.size > _MAX_STORAGE_SIZE - size:
			raise ctx.type_err(
				f'Field `{field_name}` exceeds the 32-bit storage address space'
			)
		fields[field_name] = RecordField(field_desc, size)
		size += field_desc.size
		actions_append(copy_actions, field_desc.copy_actions)

	layout = RecordLayout(size, tuple(copy_actions), fields)
	description = _RecordDesc(layout, cls)
	_install_storage_view(cls, description, tuple(fields), generic_usage)
	return description


from .numpy import populate_np_descs_if_loaded, try_handle_np  # noqa: E402


@generate_storage
class _DateTime:
	seconds: u64
	micros: u32
	has_tz: bool
	off_days: i32
	off_seconds: i32
	off_micros: i32


_dt_desc: TypeDesc[_DateTime] = _known_descs[_DateTime]


class _DateTimeDesc(TypeDesc[datetime.datetime]):
	__slots__ = ()

	def __init__(self):
		super().__init__(_dt_desc.size, _dt_desc.copy_actions)

	def get(self, slot: Slot, off: int) -> datetime.datetime:
		dt = _dt_desc.get(slot, off)

		def make_date(dt_tuple: time.struct_time, tzinfo):
			return datetime.datetime(
				year=dt_tuple.tm_year,
				month=dt_tuple.tm_mon,
				day=dt_tuple.tm_mday,
				hour=dt_tuple.tm_hour,
				minute=dt_tuple.tm_min,
				second=dt_tuple.tm_sec,
				microsecond=dt.micros,
				tzinfo=tzinfo,
			)

		if dt.has_tz:
			tz = datetime.timezone(
				datetime.timedelta(
					days=dt.off_days,
					seconds=dt.off_seconds,
					microseconds=dt.off_micros,
				)
			)
			return make_date(time.gmtime(dt.seconds), datetime.UTC).astimezone(tz)
		return make_date(time.localtime(dt.seconds), tzinfo=None)

	def set(self, slot: Slot, off: int, val: datetime.datetime) -> None:
		tz_off = None if val.tzinfo is None else val.utcoffset()
		if val.tzinfo is not None and tz_off is None:
			raise ValueError('datetime.utcoffset() returned None')
		seconds = int(val.timestamp())

		dt = _dt_desc.get(slot, off)
		dt.seconds = seconds
		dt.micros = val.microsecond
		if val.tzinfo is None:
			dt.has_tz = False
			return

		dt.has_tz = True
		tz_off = typing.cast(datetime.timedelta, tz_off)
		dt.off_days = tz_off.days
		dt.off_seconds = tz_off.seconds
		dt.off_micros = tz_off.microseconds


_known_descs[datetime.datetime] = _DateTimeDesc()
