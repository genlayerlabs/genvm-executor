import pytest
from genlayer.storage import DynArray, allow
from genlayer.storage._internal.generate import _BuilderCtx, _storage_build
from genlayer.storage.core import ROOT_SLOT_ID, InmemManager
from genlayer.types import u8, u32

from .common import SameOp


def new_vec():
	td = _storage_build(_BuilderCtx.empty(), DynArray[str])
	man = InmemManager()
	return td.get(man.get_store_slot(ROOT_SLOT_ID), 0)


def new_typed_vec(typ):
	td = _storage_build(_BuilderCtx.empty(), DynArray[typ])
	man = InmemManager()
	return td.get(man.get_store_slot(ROOT_SLOT_ID), 0)


def same_iter(li, ri):
	for l, r in zip(li, ri, strict=True):
		assert l == r


def test_len():
	lx = new_vec()
	r: list[str] = []
	op = SameOp(lx, r)
	same_iter(lx, r)
	op(len)
	op(lambda x: x.append('123'))
	op(len)
	op(lambda x: x[0])
	op(lambda x: x[-1])
	same_iter(lx, r)
	for i in range(5):
		op(str(i))
	same_iter(lx, r)
	while len(r) > 0:
		op(lambda x: x.pop(), void=True)
		same_iter(lx, r)


@pytest.mark.parametrize(
	'idx',
	[
		0,
		-1,
		4,
		-5,
	],
)
def test_setitem_int(idx: int):
	lx = new_vec()
	r: list[str] = [str(x) for x in range(10)]
	lx[:] = r
	same_iter(lx, r)

	val = 'test'
	r[idx] = val
	lx[idx] = val
	same_iter(lx, r)


@pytest.mark.parametrize(
	'idx',
	[
		slice(None, None, None),
		slice(None, None, 2),
		slice(None, None, 3),
		slice(1, 3, 2),
		slice(1, 5, 2),
		slice(1, 5, 1),
		slice(1, -1, 1),
		slice(None, None, -1),
		slice(4, 8, -1),
		slice(8, 4, -1),
		slice(8, 4, -2),
		slice(8, 3, -2),
		slice(9, 3, -2),
		slice(8, 4, -3),
		slice(9, 2, -3),
	],
)
def test_setitem_slice(idx: slice):
	lx = new_vec()
	r: list[str] = [str(x) for x in range(10)]
	lx[:] = r
	same_iter(lx, r)

	x = [str(10 + x) for x in range(5)]
	try:
		r[idx] = x
	except Exception:
		return
	lx[idx] = x
	same_iter(lx, r)


@pytest.mark.parametrize(
	'idx',
	[
		0,
		-1,
		4,
		slice(None, None, None),
		slice(None, None, 2),
		slice(None, None, 3),
		slice(1, 3, 2),
		slice(1, 5, 2),
		slice(1, 5, 1),
		slice(1, -1, 1),
		slice(None, None, -1),
		slice(4, 8, -1),
		slice(8, 4, -1),
		slice(8, 4, -2),
		slice(8, 4, -3),
	],
)
def test_getitem(idx: int | slice):
	lx = new_vec()
	r: list[str] = [str(x) for x in range(10)]
	lx[:] = r
	same_iter(lx, r)

	same_iter(lx[idx], r[idx])


@pytest.mark.parametrize(
	'idx',
	[
		0,
		-1,
		4,
		slice(None, None, None),
		slice(None, None, 2),
		slice(1, 3, 2),
		slice(1, 5, 2),
		slice(1, 5, 1),
		slice(1, -1, 1),
		slice(None, None, -1),
		slice(4, 8, -1),
		slice(8, 4, -1),
		slice(8, 4, -2),
		slice(8, 4, -3),
	],
)
def test_delitem(idx: int | slice):
	lx = new_vec()
	r: list[str] = [str(x) for x in range(10)]
	lx[:] = r
	same_iter(lx, r)

	del lx[idx]
	del r[idx]

	same_iter(lx, r)


@pytest.mark.parametrize(
	'idx',
	[
		0,
		-1,
		4,
		-5,
	],
)
def test_insert(idx: int):
	lx = new_vec()
	r: list[str] = [str(x) for x in range(10)]
	lx[:] = r
	same_iter(lx, r)

	val = 'test'
	r.insert(idx, val)
	lx.insert(idx, val)
	same_iter(lx, r)


@pytest.mark.parametrize('idx', [-100, -10, 10, 100])
def test_insert_clamps_index(idx: int):
	lx = new_vec()
	r = ['0', '1', '2']
	lx.assign(r)

	lx.insert(idx, 'test')
	r.insert(idx, 'test')

	assert list(lx) == r


def test_insert_into_empty():
	lx = new_vec()
	lx.insert(0, 'test')
	assert list(lx) == ['test']


def test_init_raises():
	with pytest.raises(TypeError):
		DynArray()


def test_assign():
	lx = new_vec()
	r = [str(x) for x in range(5)]
	lx.assign(r)
	same_iter(lx, r)
	assert len(lx) == len(r)

	r2 = ['a', 'b']
	lx.assign(r2)
	same_iter(lx, r2)
	assert len(lx) == len(r2)


def test_assign_empty():
	lx = new_vec()
	lx.append('x')
	lx.assign([])
	assert len(lx) == 0


def test_append_new_get():
	lx = new_vec()
	lx.append('hello')
	_got = lx.append_new_get()
	assert len(lx) == 2


def test_pop_empty():
	lx = new_vec()
	with pytest.raises(IndexError):
		lx.pop()


@pytest.mark.parametrize('idx', [0, 1, -1])
def test_pop_returns_indexed_element(idx: int):
	lx = new_vec()
	lx.assign(['a', 'b', 'c'])
	r = ['a', 'b', 'c']

	assert lx.pop(idx) == r.pop(idx)
	assert list(lx) == r


class Index:
	def __init__(self, value: int):
		self.value = value

	def __index__(self) -> int:
		return self.value


def test_insert_and_pop_accept_index_protocol():
	lx = new_vec()
	lx.assign(['a', 'c'])
	lx.insert(Index(1), 'b')
	assert lx.pop(Index(1)) == 'b'
	assert list(lx) == ['a', 'c']


@pytest.mark.parametrize('idx', [3, -4])
def test_pop_out_of_range(idx: int):
	lx = new_vec()
	lx.assign(['a', 'b', 'c'])
	with pytest.raises(IndexError):
		lx.pop(idx)


@allow
class Box:
	value: u32


def test_popped_storage_value_remains_a_view():
	lx = new_typed_vec(Box)
	item = lx.append_new_get()
	item.value = 1

	popped = lx.pop()
	reused = lx.append_new_get()
	reused.value = 2

	assert popped.value == 2


def test_popped_non_last_storage_value_tracks_shifted_slot():
	lx = new_typed_vec(Box)
	for value in [1, 2]:
		lx.append_new_get().value = value

	popped = lx.pop(0)

	assert popped.value == 2


def test_failed_append_exposes_new_element():
	lx = new_typed_vec(u8)
	lx.append(1)

	with pytest.raises(OverflowError):
		lx.append(256)

	assert list(lx) == [1, 0]


def test_failed_assign_leaves_array_empty():
	lx = new_typed_vec(u8)
	lx.assign([3, 4])

	with pytest.raises(OverflowError):
		lx.assign([1, 256])

	assert list(lx) == []


def test_repr():
	lx = new_vec()
	assert repr(lx) == '[]'
	lx.append('a')
	assert repr(lx) == "['a']"
	lx.append('b')
	assert repr(lx) == "['a','b']"


def test_clear():
	lx = new_vec()
	for i in range(5):
		lx.append(str(i))
	assert len(lx) == 5
	lx.clear()
	assert len(lx) == 0


def test_iter():
	lx = new_vec()
	r = [str(x) for x in range(5)]
	lx.assign(r)
	result = list(lx)
	assert result == r


@pytest.mark.parametrize('idx', [10, -11, 100])
def test_getitem_out_of_range(idx: int):
	lx = new_vec()
	lx.assign([str(x) for x in range(10)])
	with pytest.raises(IndexError):
		lx[idx]


@pytest.mark.parametrize('idx', [10, -11, 100])
def test_setitem_out_of_range(idx: int):
	lx = new_vec()
	lx.assign([str(x) for x in range(10)])
	with pytest.raises(IndexError):
		lx[idx] = 'test'


@pytest.mark.parametrize('idx', [10, -11, 100])
def test_delitem_out_of_range(idx: int):
	lx = new_vec()
	lx.assign([str(x) for x in range(10)])
	with pytest.raises(IndexError):
		del lx[idx]
