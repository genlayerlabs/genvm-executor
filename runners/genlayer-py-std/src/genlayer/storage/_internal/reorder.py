import abc
import collections.abc
import operator
import typing

from ..core import ROOT_SLOT_ID, InmemManager, Slot, TypeDesc, actions_apply_copy


class _Reorderable[T](metaclass=abc.ABCMeta):
	__slots__ = ()

	_item_desc: TypeDesc
	_storage_slot: Slot

	@abc.abstractmethod
	def __len__(self) -> int: ...

	@abc.abstractmethod
	def _items_at(self) -> tuple[Slot, int]:
		"""
		:returns: slot and offset of the first element
		"""
		...

	def _map_index(self, idx: int) -> int:
		le = len(self)
		if idx < 0:
			idx += le
		if idx < 0 or idx >= le:
			raise IndexError(f'index out of range {idx} not in 0..<{le}')
		return idx

	def swap(
		self,
		i: typing.SupportsIndex,
		j: typing.SupportsIndex,
		/,
		*,
		scratch: Slot | None = None,
	) -> None:
		"""
		Exchange two elements in place.

		Unlike ``a[i], a[j] = a[j], a[i]``, this is correct for storage-backed
		compound elements, whose views would alias each other.

		:param i: index of the first element (supports negative indexing)
		:param j: index of the second element (supports negative indexing)
		:param scratch: slot to hold one element during the exchange; it must not
			back this array. Defaults to a fresh in-memory slot
		:raises IndexError: when an index is out of range
		:raises ValueError: when ``scratch`` is a slot holding this array
		"""
		i = self._map_index(operator.index(i))
		j = self._map_index(operator.index(j))
		scratch = self._checked_scratch(scratch)
		if i == j:
			return
		slot, base = self._items_at()
		size = self._item_desc.size
		cop = self._item_desc.copy_actions
		actions_apply_copy(cop, scratch, 0, slot, base + i * size)
		actions_apply_copy(cop, slot, base + i * size, slot, base + j * size)
		actions_apply_copy(cop, slot, base + j * size, scratch, 0)

	def permute(
		self,
		order: collections.abc.Iterable[typing.SupportsIndex],
		/,
		*,
		scratch: Slot | None = None,
	) -> None:
		"""
		Rearrange elements in place so that the new element ``i`` is the old
		element ``order[i]``, as in ``[old[j] for j in order]``.

		Each element is moved once. The order is validated before anything is
		written. To shuffle, shuffle a list of indices and pass it here:
		``random.shuffle`` swaps aliasing views and corrupts compound elements.

		:param order: permutation of ``range(len(self))``
		:param scratch: slot to hold one element at a time; it must not back this
			array. Defaults to a fresh in-memory slot
		:raises TypeError: when an entry of ``order`` is not an integer
		:raises ValueError: when ``order`` is not a permutation of ``range(len(self))``,
			or ``scratch`` is a slot holding this array

		If a storage write fails, completed moves remain visible.
		"""
		scratch = self._checked_scratch(scratch)
		order = [operator.index(x) for x in order]
		n = len(self)
		if len(order) != n:
			raise ValueError(f'expected a permutation of {n} indices, got {len(order)}')
		pending = [False] * n
		for x in order:
			if not 0 <= x < n or pending[x]:
				raise ValueError(f'not a permutation of range({n})')
			pending[x] = True
		if n < 2:
			return

		slot, base = self._items_at()
		size = self._item_desc.size
		cop = self._item_desc.copy_actions
		if cop == [size]:
			old = memoryview(slot.read(base, n * size))
			slot.write(base, b''.join(old[j * size : (j + 1) * size] for j in order))
			return

		for start in range(n):
			if not pending[start]:
				continue
			pending[start] = False
			if order[start] == start:
				continue
			actions_apply_copy(cop, scratch, 0, slot, base + start * size)
			dst = start
			while (src := order[dst]) != start:
				pending[src] = False
				actions_apply_copy(cop, slot, base + dst * size, slot, base + src * size)
				dst = src
			actions_apply_copy(cop, slot, base + dst * size, scratch, 0)

	def sort(
		self,
		*,
		key: typing.Callable[[T], typing.Any] | None = None,
		reverse: bool = False,
		scratch: Slot | None = None,
	) -> None:
		"""
		Sort in place, stably, with the semantics of :py:meth:`list.sort`.

		``key`` is called once per element, before anything is written, so an
		exception from it or from a comparison leaves the array unchanged. Unlike
		``a[:] = sorted(a)``, this is correct for compound elements.

		:param key: function computing the sort key of an element
		:param reverse: sort in descending order
		:param scratch: same as for :py:meth:`permute`
		"""
		slot, base = self._items_at()
		desc = self._item_desc
		items: list[T] = [desc.get(slot, base + i * desc.size) for i in range(len(self))]
		keys: list[typing.Any] = items if key is None else [key(x) for x in items]
		order = sorted(range(len(keys)), key=lambda i: keys[i], reverse=reverse)
		self.permute(order, scratch=scratch)

	def reverse(self, *, scratch: Slot | None = None) -> None:
		"""
		Reverse in place.

		:param scratch: same as for :py:meth:`permute`
		"""
		self.permute(range(len(self) - 1, -1, -1), scratch=scratch)

	def _checked_scratch(self, scratch: Slot | None) -> Slot:
		if scratch is None:
			return InmemManager().get_store_slot(ROOT_SLOT_ID)
		if scratch == self._storage_slot or scratch == self._items_at()[0]:
			raise ValueError('scratch must not be a slot holding this array')
		return scratch
