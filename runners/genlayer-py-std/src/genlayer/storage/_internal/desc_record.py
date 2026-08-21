import collections.abc
import dataclasses
import typing

import genlayer._internal.reflect as reflect

from ..core import CopyAction, Slot, TypeDesc, _WithStorageSlot, actions_apply_copy


@dataclasses.dataclass(frozen=True, slots=True)
class RecordField:
	desc: TypeDesc
	offset: int


class _FrozenFields(collections.abc.Mapping[str, RecordField]):
	__slots__ = ('_data',)

	def __init__(self, fields: typing.Mapping[str, RecordField]):
		self._data = dict(fields)

	def __getitem__(self, key: str) -> RecordField:
		return self._data[key]

	def __iter__(self) -> typing.Iterator[str]:
		return iter(self._data)

	def __len__(self) -> int:
		return len(self._data)

	def __reduce__(self):
		return type(self), (self._data,)


@dataclasses.dataclass(frozen=True, slots=True)
class RecordLayout:
	size: int
	copy_actions: tuple[CopyAction, ...]
	fields: typing.Mapping[str, RecordField]

	def __init__(
		self,
		size: int,
		copy_actions: tuple[CopyAction, ...],
		fields: dict[str, RecordField],
	):
		object.__setattr__(self, 'size', size)
		object.__setattr__(self, 'copy_actions', copy_actions)
		object.__setattr__(self, 'fields', _FrozenFields(fields))


class RecordExtraFields(_WithStorageSlot, typing.Protocol):
	__type_desc__: '_RecordDesc'


class _RecordDesc[T: RecordExtraFields](TypeDesc[T]):
	__slots__ = ('cls', 'hsh', 'layout', 'props')

	def __init__(self, layout: RecordLayout, cls: typing.Type[T]):
		TypeDesc.__init__(self, layout.size, list(layout.copy_actions))
		self.layout = layout
		self.props = {
			name: (field.desc, field.offset) for name, field in layout.fields.items()
		}
		self.cls = cls
		self.hsh = hash((('_RecordDesc', self.size), *sorted(self.props.items())))

	def get(self, slot: Slot, off: int) -> T:
		slf = self.cls.__new__(self.cls)
		slf._storage_slot = slot
		slf._off = off
		slf.__type_desc__ = self
		return slf

	def set(self, slot: Slot, off: int, val: T) -> None:
		err = f'incompatible storage type: `{reflect.repr_type(self.cls)}` <- `{reflect.repr_type(type(val))}`'
		if not hasattr(val, '__type_desc__') or val.__type_desc__ != self:
			raise TypeError(err)
		actions_apply_copy(self.copy_actions, slot, off, val._storage_slot, val._off)

	def __eq__(self, other: object) -> bool:
		if not isinstance(other, _RecordDesc):
			return False
		if other is self:
			return True
		return (
			other.hsh == self.hsh and other.size == self.size and other.props == self.props
		)

	def __hash__(self) -> int:
		return self.hsh
