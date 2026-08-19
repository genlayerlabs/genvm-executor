import pickle
import typing

import pytest
from genlayer.storage import Array, DynArray, allow, inmem_allocate
from genlayer.storage._internal.generate import (
	STORAGE_PATCHED_ATTR,
	GenerationError,
	_BuilderCtx,
	_storage_build,
)
from genlayer.types import u8, u32, u64


@allow
class Box[T]:
	value: T

	def __init__(self, value: T):
		self.value = value


@allow
class Inner[T]:
	value: T


@allow
class Outer[T]:
	inner: Inner[T]


@allow
class Recursive:
	child: 'Recursive'


@allow
class Base:
	base: u32


@allow
class Derived(Base):
	derived: u64


@allow
class GenericBase[T]:
	base: T


@allow
class GenericDerived[T](GenericBase[T]):
	derived: T


@allow
class ConcreteDerived(GenericBase[u32]):
	derived: u64


@allow
class ConcreteMiddle(GenericBase[u32]):
	pass


@allow
class DeepConcrete(ConcreteMiddle):
	derived: u64


@allow
class Phantom[T]:
	value: u32


@allow
class InheritedPhantom[T](GenericBase[u32]):
	derived: u64


@allow
class MetadataPhantom[T]:
	value: typing.Annotated[u32, T]


def test_concrete_layout_is_cached():
	ctx = _BuilderCtx.empty()
	first = _storage_build(ctx, DynArray[u32])
	second = _storage_build(ctx, DynArray[u32])

	assert first is second


def test_generic_specializations_have_distinct_cached_layouts():
	ctx = _BuilderCtx.empty()
	u32_desc = _storage_build(ctx, Box[u32])
	u64_desc = _storage_build(ctx, Box[u64])

	assert u32_desc is _storage_build(ctx, Box[u32])
	assert u64_desc is _storage_build(ctx, Box[u64])
	assert u32_desc is not u64_desc
	assert u32_desc.size == 4
	assert u64_desc.size == 8


def test_generic_views_use_the_instance_layout():
	u32_box = inmem_allocate(Box[u32], 0x01020304)
	u64_box = inmem_allocate(Box[u64], 0x0102030405060708)

	assert u32_box.value == 0x01020304
	assert u64_box.value == 0x0102030405060708


def test_nested_type_vars_are_resolved_by_identity():
	outer = inmem_allocate(Outer[u32])
	inner = inmem_allocate(Inner[u32])
	inner.value = 42
	outer.inner = inner

	assert outer.inner.value == 42
	assert outer.__type_desc__.layout.fields['inner'].desc.size == 4


def test_storage_view_is_installed_once():
	_storage_build(_BuilderCtx.empty(), Box[u32])
	first_property = Box.value
	_storage_build(_BuilderCtx.empty(), Box[u64])

	assert Box.value is first_property
	assert Box.__dict__[STORAGE_PATCHED_ATTR] is True


def test_derived_class_allocates_its_own_layout():
	_storage_build(_BuilderCtx.empty(), Base)
	_storage_build(_BuilderCtx.empty(), Derived)
	value = Derived()
	value.base = 1
	value.derived = 2

	assert value.base == 1
	assert value.derived == 2
	assert value.__type_desc__.size == 12


def test_inherited_type_vars_are_resolved_by_identity():
	value = inmem_allocate(GenericDerived[u32])
	value.base = 1
	value.derived = 2

	assert value.base == 1
	assert value.derived == 2
	assert value.__type_desc__.size == 8


def test_concretely_specialized_generic_base_is_resolved():
	_storage_build(_BuilderCtx.empty(), ConcreteDerived)
	value = ConcreteDerived()
	value.base = 1
	value.derived = 2

	assert value.base == 1
	assert value.derived == 2
	assert value.__type_desc__.size == 12


def test_specialization_survives_non_generic_intermediate_base():
	_storage_build(_BuilderCtx.empty(), DeepConcrete)
	value = DeepConcrete()
	value.base = 1
	value.derived = 2

	assert value.base == 1
	assert value.derived == 2
	assert value.__type_desc__.size == 12


def test_storage_view_remains_picklable():
	value = Derived()
	value.base = 1
	value.derived = 2

	loaded = pickle.loads(pickle.dumps(value))

	assert loaded.base == 1
	assert loaded.derived == 2


def test_generic_class_requires_explicit_allocation():
	_storage_build(_BuilderCtx.empty(), Box[u32])

	with pytest.raises(GenerationError, match='inmem_allocate') as raised:
		Box(1)

	assert raised.value.__notes__[0].startswith('during building type `Box[')
	assert raised.value.__notes__[1:] == [
		'declared generic variables: T',
		'during processing field `value: T`',
		'during building type `T`',
		'due to usage of `T`',
		'in class `Box`',
	]


def test_phantom_generic_can_be_constructed_directly():
	_storage_build(_BuilderCtx.empty(), Phantom[u64])
	value = Phantom()
	value.value = 42

	assert value.value == 42


def test_fixed_inherited_generic_can_be_constructed_directly():
	_storage_build(_BuilderCtx.empty(), InheritedPhantom[u8])
	value = InheritedPhantom()
	value.base = 1
	value.derived = 2

	assert value.base == 1
	assert value.derived == 2
	assert value.__type_desc__.size == 12


def test_type_var_in_annotated_metadata_does_not_affect_layout():
	_storage_build(_BuilderCtx.empty(), MetadataPhantom[u64])
	value = MetadataPhantom()
	value.value = 42

	assert value.value == 42


def test_recursive_layout_is_rejected():
	with pytest.raises(GenerationError, match='Recursive storage type'):
		_storage_build(_BuilderCtx.empty(), Recursive)


@pytest.mark.parametrize('size', [typing.Literal[0], typing.Literal[-1]])
def test_array_size_must_be_positive(size):
	with pytest.raises(GenerationError, match='strictly positive'):
		_storage_build(_BuilderCtx.empty(), Array[u8, size])
