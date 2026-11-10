import random
import typing

import pytest
from genlayer.storage import Array, DynArray, TreeMap, allow
from genlayer.storage._internal.generate import _BuilderCtx, _storage_build
from genlayer.storage.core import ROOT_SLOT_ID, InmemManager
from genlayer.types import u32


@allow
class P:
	k: u32
	s: str

	def __init__(self, k: int, s: str):
		self.k = k
		self.s = s


def new_view(typ):
	td = _storage_build(_BuilderCtx.empty(), typ)
	return td.get(InmemManager().get_store_slot(ROOT_SLOT_ID), 0)


def p_dump(arr) -> list[tuple[int, str]]:
	return [(p.k, p.s) for p in arr]


PS = [
	(3, 'c'),
	(1, 'a'),
	(2, 'b'),
	(1, 'z'),
	(5, 'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee'),
]


def new_ps():
	arr = new_view(DynArray[P])
	for k, s in PS:
		arr.append(P(k, s))
	return arr


def new_nested():
	arr = new_view(DynArray[DynArray[u32]])
	for le in [3, 0, 7, 1]:
		arr.append(list(range(le * 10, le * 10 + le)))
	return arr


def nested_dump(arr) -> list[list[int]]:
	return [list(x) for x in arr]


def new_maps():
	arr = new_view(DynArray[TreeMap[str, u32]])
	for i in range(4):
		m = arr.append_new_get()
		for j in range(i + 1):
			m[f'{i}.{j}'] = j
	return arr


def maps_dump(arr) -> list[dict[str, int]]:
	return [dict(m.items()) for m in arr]


DynCase = tuple[typing.Callable[[], typing.Any], typing.Callable[[typing.Any], list]]

DYN_CASES: dict[str, DynCase] = {
	'flat': (lambda: _filled(DynArray[u32], [5, 3, 9, 3, 1]), list),
	'str': (lambda: _filled(DynArray[str], ['bb', 'a', 'x' * 100, '', 'a']), list),
	'record': (new_ps, p_dump),
	'nested': (new_nested, nested_dump),
	'tree_map': (new_maps, maps_dump),
}


def _filled(typ, vals):
	arr = new_view(typ)
	arr.extend(vals)
	return arr


@pytest.fixture(params=list(DYN_CASES))
def dyn_case(request) -> DynCase:
	return DYN_CASES[request.param]


def test_reverse(dyn_case):
	mk, dump = dyn_case
	arr = mk()
	expected = dump(arr)[::-1]
	arr.reverse()
	assert dump(arr) == expected


@pytest.mark.parametrize('i,j', [(0, 1), (1, 0), (0, -1), (2, 2), (-2, 1)])
def test_swap(dyn_case, i, j):
	mk, dump = dyn_case
	arr = mk()
	expected = dump(arr)
	expected[i], expected[j] = expected[j], expected[i]
	arr.swap(i, j)
	assert dump(arr) == expected


def test_swap_out_of_range(dyn_case):
	mk, dump = dyn_case
	arr = mk()
	before = dump(arr)
	with pytest.raises(IndexError):
		arr.swap(0, len(arr))
	with pytest.raises(IndexError):
		arr.swap(-len(arr) - 1, 0)
	assert dump(arr) == before


@pytest.mark.parametrize('seed', range(5))
def test_permute(dyn_case, seed):
	mk, dump = dyn_case
	arr = mk()
	order = list(range(len(arr)))
	random.Random(seed).shuffle(order)
	old = dump(arr)
	arr.permute(order)
	assert dump(arr) == [old[j] for j in order]


@pytest.mark.parametrize(
	'order,exc',
	[
		([0, 1], ValueError),
		([0, 1, 2, 3, 4, 5], ValueError),
		([0, 0, 1, 2, 3], ValueError),
		([1, 2, 3, 4, 5], ValueError),
		([-1, 0, 1, 2, 3], ValueError),
		([0, 1, 2, 3, 'x'], TypeError),
	],
)
def test_permute_rejects_non_permutations(dyn_case, order, exc):
	mk, dump = dyn_case
	arr = mk()
	before = dump(arr)
	with pytest.raises(exc):
		arr.permute(order)
	assert dump(arr) == before


def test_sort_flat_matches_list():
	vals = [5, 3, 9, 3, 1, 0, 7]
	for reverse in [False, True]:
		arr = _filled(DynArray[u32], vals)
		arr.sort(reverse=reverse)
		assert list(arr) == sorted(vals, reverse=reverse)


@pytest.mark.parametrize('reverse', [False, True])
def test_sort_record_by_key_is_stable(reverse):
	arr = new_ps()
	arr.sort(key=lambda p: p.k, reverse=reverse)
	assert p_dump(arr) == sorted(PS, key=lambda x: x[0], reverse=reverse)


def test_sort_nested_by_len():
	arr = new_nested()
	expected = sorted(nested_dump(arr), key=len)
	arr.sort(key=len)
	assert nested_dump(arr) == expected


def test_sort_failing_key_leaves_array_intact():
	arr = new_ps()

	def key(p):
		if p.k == 2:
			raise RuntimeError('boom')
		return p.k

	with pytest.raises(RuntimeError):
		arr.sort(key=key)
	assert p_dump(arr) == PS


def test_sort_failing_comparison_leaves_array_intact():
	arr = new_ps()
	with pytest.raises(TypeError):
		arr.sort(key=lambda p: p.s if p.k == 2 else p.k)
	assert p_dump(arr) == PS


def test_sort_by_view_is_rejected_before_writes():
	arr = new_nested()
	before = nested_dump(arr)
	with pytest.raises(TypeError):
		arr.sort()
	assert nested_dump(arr) == before


@pytest.mark.parametrize('reverse', [False, True])
def test_sort_flat_ties_are_stable(reverse):
	vals = [21, 10, 31, 11, 20, 30]
	arr = _filled(DynArray[u32], vals)
	arr.sort(key=lambda x: x // 10, reverse=reverse)
	assert list(arr) == sorted(vals, key=lambda x: x // 10, reverse=reverse)


def test_sort_is_keyword_only():
	arr = _filled(DynArray[u32], [2, 1])
	with pytest.raises(TypeError):
		arr.sort(None)  # type: ignore


def test_scratch_is_keyword_only():
	arr = _filled(DynArray[u32], [2, 1])
	with pytest.raises(TypeError):
		arr.swap(0, 1, InmemManager().get_store_slot(ROOT_SLOT_ID))  # type: ignore


def test_permute_long_cycle_and_fixed_points(dyn_case):
	mk, dump = dyn_case
	arr = mk()
	order = {4: [2, 1, 3, 0], 5: [3, 1, 0, 4, 2]}[len(arr)]
	old = dump(arr)
	arr.permute(iter(order))
	assert dump(arr) == [old[j] for j in order]


@pytest.mark.parametrize('typ', [u32, str])
def test_permute_large(typ):
	n = 1000
	vals = [typ(i) for i in range(n)]
	arr = _filled(DynArray[typ], vals)
	order = list(range(n))
	random.Random(0).shuffle(order)
	arr.permute(order)
	assert list(arr) == [vals[j] for j in order]
	arr.sort(key=int)
	assert list(arr) == vals


def test_scratch_holding_the_array_is_rejected():
	arr = new_ps()
	for scratch in [arr._storage_slot, arr._storage_slot.indirect(arr._off)]:
		with pytest.raises(ValueError):
			arr.swap(0, 1, scratch=scratch)
		with pytest.raises(ValueError):
			arr.permute([1, 0, 2, 3, 4], scratch=scratch)
	assert p_dump(arr) == PS


def test_explicit_scratch_is_reusable():
	scratch = InmemManager().get_store_slot(ROOT_SLOT_ID)
	ps = new_ps()
	nested = new_nested()
	ps.swap(0, 4, scratch=scratch)
	nested.swap(0, 2, scratch=scratch)
	nested.sort(key=len, reverse=True, scratch=scratch)
	ps.sort(key=lambda p: p.s, scratch=scratch)
	ps.reverse(scratch=scratch)
	assert p_dump(ps) == sorted(PS, key=lambda x: x[1])[::-1]
	assert [len(x) for x in nested] == [7, 3, 1, 0]


def test_empty_and_single():
	for vals in [[], [(1, 'a')]]:
		arr = new_view(DynArray[P])
		for k, s in vals:
			arr.append(P(k, s))
		arr.reverse()
		arr.sort(key=lambda p: p.k)
		arr.permute(range(len(vals)))
		assert p_dump(arr) == vals


ARR_P = Array[P, typing.Literal[4]]


def new_arr_p():
	arr = new_view(ARR_P)
	for i, (k, s) in enumerate(PS[:4]):
		arr[i] = P(k, s)
	return arr


def test_array_swap_permute_sort():
	arr = new_arr_p()
	arr.swap(0, -1)
	expected = list(PS[:4])
	expected[0], expected[-1] = expected[-1], expected[0]
	assert p_dump(arr) == expected

	arr.permute([2, 0, 3, 1])
	expected = [expected[j] for j in [2, 0, 3, 1]]
	assert p_dump(arr) == expected

	arr.sort(key=lambda p: p.s, reverse=True)
	assert p_dump(arr) == sorted(expected, key=lambda x: x[1], reverse=True)


def test_array_slice_view_reorders_only_its_range():
	arr = new_arr_p()
	arr[1:3].swap(0, 1)
	assert p_dump(arr) == [PS[0], PS[2], PS[1], PS[3]]


def test_array_flat_sort():
	arr = new_view(Array[u32, typing.Literal[5]])
	for i, v in enumerate([4, 2, 8, 2, 0]):
		arr[i] = v
	arr.sort()
	assert list(arr) == [0, 2, 2, 4, 8]


@pytest.mark.parametrize(
	'typ,vals,dump',
	[
		(str, ['ccc', '', 'a' * 100], list),
		(DynArray[u32], [[1, 2, 3, 4, 5], [], [9]], lambda a: [list(x) for x in a]),
	],
)
def test_array_compound_elements(typ, vals, dump):
	arr = new_view(Array[typ, typing.Literal[3]])
	for i, v in enumerate(vals):
		arr[i] = v
	arr.reverse()
	assert dump(arr) == vals[::-1]
	arr.swap(0, 1)
	assert dump(arr) == [vals[1], vals[2], vals[0]]
	arr.sort(key=len)
	assert dump(arr) == sorted(vals, key=len)
