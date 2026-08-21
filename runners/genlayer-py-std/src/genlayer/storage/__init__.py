"""
Persistent storage module for GenLayer contracts.

This module provides:
- ``DynArray``: Dynamic-length arrays
- ``Array``: Fixed-size arrays
- ``TreeMap``: Tree-based key-value storage
- ``allow``: Decorator for storage-enabled classes
- ``inmem_allocate``: In-memory allocation utility
- ``Root``: Root storage class
"""

__all__ = (
	'DynArray',
	'Array',
	'TreeMap',
	'Comparable',
	'allow',
	'inmem_allocate',
	'Pickled',
	'Root',
	'ROOT_SLOT_ID',
	'Slot',
	'Manager',
	'Indirection',
	'VLA',
)

import typing

from ._internal.generate import (
	ORIGINAL_INIT_ATTR,
	_BuilderCtx,
	_storage_build,
	allow,
)
from .array import Array
from .core import ROOT_SLOT_ID, VLA, Indirection, InmemManager, Manager, Slot
from .dyn_array import DynArray
from .root import Root
from .tree_map import Comparable, TreeMap


def inmem_allocate[T](t: typing.Type[T], /, *init_args, **init_kwargs) -> T:
	"""
	Allocate a storage type in memory (useful for testing).

	:param t: storage-allowed type to allocate
	:param init_args: positional arguments forwarded to ``__init__``
	:param init_kwargs: keyword arguments forwarded to ``__init__``
	:returns: new in-memory instance of the given type
	"""
	td = _storage_build(_BuilderCtx.empty(), t)
	man = InmemManager()

	instance = td.get(man.get_store_slot(ROOT_SLOT_ID), 0)

	init = getattr(td, 'cls', None)
	if init is None:
		init = getattr(t, '__init__', None)
	else:
		init = getattr(init, '__init__', None)
	if init is not None:
		if hasattr(init, ORIGINAL_INIT_ATTR):
			init = getattr(init, ORIGINAL_INIT_ATTR)
		init(instance, *init_args, **init_kwargs)

	return instance


def cast_slot[T](t: typing.Type[T], manager: Manager, slot: bytes, offset: int, /) -> T:
	"""
	Unsafely casts a storage slot to the given type. Use with caution.
	"""
	slt = manager.get_store_slot(slot)
	return slt.cast(t, offset)


def copy_to_memory[T](val: T, /) -> T:
	"""
	Deep-copy a storage-enabled class instance into a new in-memory instance.

	:param val: storage-backed class instance to copy
	:returns: independent in-memory copy
	:raises TypeError: when val is not a storage-enabled class instance
	"""
	td = getattr(val, '__type_desc__', None)
	if td is None:
		raise TypeError(f'expected a storage-enabled class, got {type(val).__name__}')

	man = InmemManager()
	slot = man.get_store_slot(ROOT_SLOT_ID)

	td.set(slot, 0, val)

	return td.get(slot, 0)


import pickle  # noqa: E402


@allow
class Pickled[T]:
	"""
	Storage wrapper that persists arbitrary Python objects via pickle serialization.
	"""

	_data: bytes

	def load(self) -> T:
		"""
		Deserialize and return the stored value.

		:returns: the unpickled object
		"""
		return pickle.loads(self._data)

	def store(self, val: T, /) -> None:
		"""
		Serialize and persist the given value.

		:param val: object to pickle and store

		Serialization completes before storage is changed. If serialization
		fails, the previous value remains intact. A later storage-write failure
		may leave the byte payload only partially updated.
		"""
		self._data = pickle.dumps(val)
