import pytest
from genlayer.storage import DynArray, allow
from genlayer.storage._internal.generate import _BuilderCtx, _storage_build
from genlayer.storage.core import ROOT_SLOT_ID, InmemManager, Slot
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
	'idx, count',
	[
		(slice(0, 5, 2), 3),
		(slice(None, None, 3), 4),
		(slice(1, 10, 4), 3),
		(slice(9, 2, -3), 3),
		(slice(None, None, -1), 10),
	],
)
def test_setitem_extended_slice(idx: slice, count: int):
	lx = new_vec()
	r: list[str] = [str(x) for x in range(10)]
	lx[:] = r

	x = [str(10 + i) for i in range(count)]
	r[idx] = x
	lx[idx] = x
	same_iter(lx, r)


@pytest.mark.parametrize(
	'idx', [slice(0, 5, 2), slice(None, None, -1), slice(9, 2, -3)]
)
def test_setitem_extended_slice_size_mismatch(idx: slice):
	lx = new_vec()
	r: list[str] = [str(x) for x in range(10)]
	lx[:] = r

	with pytest.raises(ValueError):
		lx[idx] = ['a', 'b']
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


@pytest.mark.parametrize('separate_view', [False, True])
@pytest.mark.parametrize('values', [[], ['a', 'b', 'c']])
def test_assign_same_storage_is_noop(separate_view, values):
	lx = new_vec().assign(values)
	source = lx
	if separate_view:
		slot = lx._storage_slot
		source = Slot(slot.id, slot.manager).cast(DynArray[str], lx._off)
		assert source is not lx
	assert lx.assign(source) is lx
	assert list(lx) == values


@pytest.mark.parametrize('location', ['manager', 'slot', 'offset'])
def test_assign_distinct_storage(location):
	lx = new_vec().assign(['a'])
	slot = lx._storage_slot
	if location == 'manager':
		source = new_vec()
	elif location == 'slot':
		source = slot.indirect(100).cast(DynArray[str], 0)
	else:
		source = slot.cast(DynArray[str], 4)
	source.assign(['b', 'c'])
	lx.assign(source)
	assert list(lx) == list(source) == ['b', 'c']


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


@pytest.mark.parametrize('operation', ['get', 'set', 'delete'])
@pytest.mark.parametrize('index', [0, -1, 3, -4])
def test_item_operations_accept_index_protocol(operation, index):
	lx = new_vec().assign(['a', 'b', 'c'])
	expected = ['a', 'b', 'c']

	def apply(arr, idx):
		if operation == 'get':
			return arr[idx]
		if operation == 'set':
			arr[idx] = 'x'
		else:
			del arr[idx]

	try:
		result = apply(expected, index)
	except IndexError:
		with pytest.raises(IndexError):
			apply(lx, Index(index))
	else:
		assert apply(lx, Index(index)) == result
	assert list(lx) == expected


@pytest.mark.parametrize('operation', ['get', 'set', 'delete'])
@pytest.mark.parametrize('index', [1.0, '1', None, Index('1')])
def test_item_operations_reject_invalid_indices(operation, index):
	lx = new_vec().assign(['a', 'b', 'c'])
	with pytest.raises(TypeError):
		if operation == 'get':
			lx[index]
		elif operation == 'set':
			lx[index] = 'x'
		else:
			del lx[index]
	assert list(lx) == ['a', 'b', 'c']


@pytest.mark.parametrize('step', [1, 2, -1, -2, 0])
@pytest.mark.parametrize('count', [0, 2, 3, 4])
def test_setitem_slice_accepts_index_step(step, count):
	lx = new_vec()
	r = ['a', 'b', 'c']
	lx.assign(r)
	idx = slice(None, None, Index(step))
	values = [str(i) for i in range(count)]
	try:
		r[idx] = values
	except ValueError:
		with pytest.raises(ValueError):
			lx[idx] = values
	else:
		lx[idx] = values
	assert list(lx) == r


def test_setitem_slice_normalizes_step_once():
	class Once:
		def __index__(self):
			assert not getattr(self, 'called', False)
			self.called = True
			return 1

	lx = new_vec()
	lx.assign(['a', 'b', 'c'])
	lx[:: Once()] = ['x', 'y']
	assert list(lx) == ['x', 'y']


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
