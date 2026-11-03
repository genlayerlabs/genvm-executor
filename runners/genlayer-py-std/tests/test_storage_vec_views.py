"""
``DynArray.__getitem__`` returns a view of the stored element for compound element
types (storage classes, nested ``DynArray``, ``TreeMap``). Operations that read
elements and write them back into the same array must not read through a view that
an earlier write already overwrote.
"""

import pytest
from genlayer.storage import DynArray, TreeMap, allow, inmem_allocate
from genlayer.types import u32


@allow
class Item:
	owner: str
	amount: u32


@allow
class State:
	items: DynArray[Item]
	nested: DynArray[DynArray[u32]]
	maps: DynArray[TreeMap[str, u32]]
	names: DynArray[str]


def fill(st: State, n: int) -> None:
	for i in range(1, n + 1):
		it = st.items.append_new_get()
		it.owner = f'owner{i}'
		it.amount = i
		st.nested.append([i, i * 10])
		m = st.maps.append_new_get()
		m[f'k{i}'] = i
		m['common'] = i * 100
		st.names.append(f'n{i}')


def items(st: State) -> list[tuple[str, int]]:
	return [(x.owner, int(x.amount)) for x in st.items]


def expected_items(vals: list[int]) -> list[tuple[str, int]]:
	return [(f'owner{v}', v) for v in vals]


def nested(st: State) -> list[list[int]]:
	return [[int(y) for y in x] for x in st.nested]


def maps(st: State) -> list[list[tuple[str, int]]]:
	return [sorted((k, int(v)) for k, v in m.items()) for m in st.maps]


def new_state(n: int) -> State:
	st = inmem_allocate(State)
	fill(st, n)
	return st


@pytest.mark.parametrize('n', range(8))
def test_reverse_storage_class_elements(n: int):
	st = new_state(n)
	st.items.reverse()
	assert items(st) == expected_items(list(range(n, 0, -1)))
	assert len(st.items) == n


@pytest.mark.parametrize('n', range(8))
def test_reverse_nested_dyn_array_elements(n: int):
	st = new_state(n)
	st.nested.reverse()
	assert nested(st) == [[i, i * 10] for i in range(n, 0, -1)]


@pytest.mark.parametrize('n', range(6))
def test_reverse_tree_map_elements(n: int):
	st = new_state(n)
	st.maps.reverse()
	assert maps(st) == [[('common', i * 100), (f'k{i}', i)] for i in range(n, 0, -1)]


@pytest.mark.parametrize('n', range(6))
def test_reverse_plain_elements_still_works(n: int):
	st = new_state(n)
	st.names.reverse()
	assert list(st.names) == [f'n{i}' for i in range(n, 0, -1)]


def test_reverse_twice_restores_order():
	st = new_state(5)
	st.items.reverse()
	st.items.reverse()
	assert items(st) == expected_items([1, 2, 3, 4, 5])


@pytest.mark.parametrize('n', range(7))
def test_slice_assign_reversed_self(n: int):
	st = new_state(n)
	st.items[:] = st.items[::-1]
	assert items(st) == expected_items(list(range(n, 0, -1)))
	assert len(st.items) == n


def test_slice_assign_shifting_left_from_self():
	st = new_state(5)
	st.items[0:4] = st.items[1:5]
	assert items(st) == expected_items([2, 3, 4, 5, 5])


def test_slice_assign_shifting_right_from_self():
	st = new_state(5)
	st.items[1:5] = st.items[0:4]
	assert items(st) == expected_items([1, 1, 2, 3, 4])


@pytest.mark.parametrize(
	'start, stop, src',
	[
		(1, 1, slice(None, None, -1)),  # insert a copy of the reversed array in the middle
		(1, 3, slice(None, None, -1)),  # replace with something longer
		(0, 4, slice(0, 2)),  # replace with something shorter
		(2, 2, slice(None)),  # insert a copy of the whole array
	],
)
def test_slice_assign_changing_length_from_self(start: int, stop: int, src: slice):
	st = new_state(4)
	model = [1, 2, 3, 4]

	st.items[start:stop] = st.items[src]
	model[start:stop] = model[src]

	assert items(st) == expected_items(model)
	assert len(st.items) == len(model)


@pytest.mark.parametrize('n', range(6))
def test_assign_reversed_self(n: int):
	st = new_state(n)
	st.items.assign(st.items[::-1])
	assert items(st) == expected_items(list(range(n, 0, -1)))


def test_slice_assign_nested_dyn_array_reversed_self():
	st = new_state(4)
	st.nested[:] = st.nested[::-1]
	assert nested(st) == [[4, 40], [3, 30], [2, 20], [1, 10]]


def test_slice_assign_from_another_array_is_unchanged():
	st = new_state(3)
	other = inmem_allocate(State)
	for i in (3, 2, 1):
		it = other.items.append_new_get()
		it.owner = f'owner{i}'
		it.amount = i

	st.items[:] = other.items[:]

	assert items(st) == expected_items([3, 2, 1])
	assert items(other) == expected_items([3, 2, 1])


def test_slice_assign_plain_values_is_unchanged():
	st = new_state(4)
	st.names[1:3] = ['x', 'y', 'z']
	assert list(st.names) == ['n1', 'x', 'y', 'z', 'n4']
