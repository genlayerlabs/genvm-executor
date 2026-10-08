import typing

import pytest
from genlayer.storage import Array, DynArray, TreeMap, allow, inmem_allocate
from genlayer.storage.core import Indirection, Slot
from genlayer.types import u32


def new_map():
	return inmem_allocate(TreeMap[str, str])


def same_iter(li, ri):
	for l, r in zip(li, ri, strict=True):
		assert l == r


def test_construct():
	r = {str(i): str(i) + str(i) for i in range(10)}
	m = new_map()
	m.update(r)

	same_iter(m.items(), r.items())


@pytest.mark.parametrize('key', ['1', '-1', '2', '10'])
def test_contains(key: str):
	r = {str(i): str(i) + str(i) for i in range(10)}
	m = new_map()
	m.update(r)

	assert (key in m) == (key in r)


@pytest.mark.parametrize(
	'key,dflt', [(key, dflt) for key in ['1', '-1', '2', '10'] for dflt in [None, 'dflt']]
)
def test_get_dflt(key: str, dflt):
	r = {str(i): str(i) + str(i) for i in range(10)}
	m = new_map()
	m.update(r)

	assert m.get(key, dflt) == r.get(key, dflt)


@pytest.mark.parametrize('key', ['1', '-1', '2', '10'])
def test_get(key: str):
	r = {str(i): str(i) + str(i) for i in range(10)}
	m = new_map()
	m.update(r)

	assert m.get(key) == r.get(key)


@pytest.mark.parametrize('key', ['1', '-1', '2', '10', '1000'])
def test_set(key: str):
	r = {str(i): str(i) + str(i) for i in range(10)}
	m = new_map()
	m.update(r)

	m[key] = 'test'
	r[key] = 'test'

	same_iter(sorted(m.items()), sorted(r.items()))


@pytest.mark.parametrize('key', ['1', '2', '9'])
def test_del(key: str):
	r = {str(i): str(i) + str(i) for i in range(10)}
	m = new_map()
	m.update(r)

	del m[key]
	del r[key]

	same_iter(sorted(m.items()), sorted(r.items()))


def test_repr():
	v = {
		'b': 'русские буквы',
		'a': 'c',
	}

	m = new_map()
	m.update(v)

	assert repr(m) == "{'a':'c','b':'русские буквы'}"


def test_repr_empty():
	m = new_map()
	assert repr(m) == '{}'


def test_len():
	m = new_map()
	assert len(m) == 0
	m['a'] = 'x'
	assert len(m) == 1
	m['b'] = 'y'
	assert len(m) == 2
	del m['a']
	assert len(m) == 1


def test_clear():
	r = {str(i): str(i) for i in range(10)}
	m = new_map()
	m.update(r)
	assert len(m) == 10
	m.clear()
	assert len(m) == 0
	assert list(m) == []


def test_getitem():
	m = new_map()
	m['a'] = 'val_a'
	assert m['a'] == 'val_a'


def test_getitem_missing():
	m = new_map()
	with pytest.raises(KeyError):
		m['missing']


def test_del_missing():
	m = new_map()
	m['a'] = 'x'
	with pytest.raises(KeyError):
		del m['missing']


def test_iter():
	r = {str(i): str(i) for i in range(10)}
	m = new_map()
	m.update(r)
	assert sorted(m) == sorted(r)


def test_assign():
	m = new_map()
	m['pre'] = 'existing'
	r = {'a': '1', 'b': '2', 'c': '3'}
	m.assign(r)
	assert len(m) == len(r)
	same_iter(sorted(m.items()), sorted(r.items()))


@pytest.mark.parametrize('separate_view', [False, True])
@pytest.mark.parametrize('values', [{}, {'a': '1', 'b': '2', 'c': '3'}])
def test_assign_same_storage_is_noop(separate_view, values, monkeypatch):
	m = new_map().assign(values)
	source = m
	if separate_view:
		slot = m._storage_slot
		source = Slot(slot.id, slot.manager).cast(TreeMap[str, str], m._off)
		assert source is not m

	def unexpected_write(*args):
		pytest.fail('same-storage assignment must not write')

	monkeypatch.setattr(type(m._storage_slot.manager), 'do_write', unexpected_write)
	assert m.assign(source) is m
	assert dict(m.items()) == values


@pytest.mark.parametrize('location', ['manager', 'slot', 'offset'])
def test_assign_distinct_storage(location):
	m = new_map().assign({'old': 'value'})
	slot = m._storage_slot
	if location == 'manager':
		source = new_map()
	elif location == 'slot':
		source = slot.indirect(100).cast(TreeMap[str, str], 0)
	else:
		source = slot.cast(TreeMap[str, str], m.__type_desc__.size)
	values = {'a': '1', 'b': '2'}
	source.assign(values)
	assert m.assign(source) is m
	assert dict(m.items()) == dict(source.items()) == values


def test_compute_if_absent_missing():
	m = new_map()
	result = m.compute_if_absent('k', lambda: 'new_val')
	assert result == 'new_val'
	assert m['k'] == 'new_val'


def test_compute_if_absent_existing():
	m = new_map()
	m['k'] = 'old_val'
	result = m.compute_if_absent('k', lambda: 'new_val')
	assert result == 'old_val'
	assert m['k'] == 'old_val'


def test_compute_if_absent_supplier_error_does_not_mutate():
	m = new_map()
	m['before'] = 'value'

	def fail():
		raise ValueError('supplier failed')

	with pytest.raises(ValueError, match='supplier failed'):
		m.compute_if_absent('missing', fail)

	assert list(m.items()) == [('before', 'value')]


@pytest.mark.parametrize('separate_view', [False, True])
@pytest.mark.parametrize('values', [{}, {'existing': 'value'}])
@pytest.mark.parametrize(
	'operation',
	['insert', 'overwrite', 'delete', 'clear', 'assign', 'default', 'compute'],
)
def test_supplier_cannot_mutate_same_map(separate_view, values, operation):
	m = new_map().assign(values)
	alias = m
	if separate_view:
		slot = m._storage_slot
		alias = Slot(slot.id, slot.manager).cast(TreeMap[str, str], m._off)

	def supplier():
		if operation == 'insert':
			alias['other'] = 'new'
		elif operation == 'overwrite':
			alias['existing'] = 'new'
		elif operation == 'delete':
			del alias['existing']
		elif operation == 'clear':
			alias.clear()
		elif operation == 'assign':
			alias.assign({'other': 'new'})
		elif operation == 'default':
			alias.get_or_insert_default('other')
		else:
			alias.compute_if_absent('other', lambda: 'new')
		return 'result'

	with pytest.raises(RuntimeError, match='supplier callback'):
		m.compute_if_absent('missing', supplier)
	assert dict(m.items()) == values
	assert len(m) == len(values)
	assert m.compute_if_absent('missing', lambda: 'result') == 'result'


def test_supplier_can_read_same_map_and_mutate_other_map():
	m = new_map().assign({'existing': 'value'})
	other = new_map()

	def supplier():
		assert m['existing'] == 'value'
		assert m.get_or_insert_default('existing') == 'value'
		assert m.compute_if_absent('existing', lambda: pytest.fail('called')) == 'value'
		other.compute_if_absent('key', lambda: 'other')
		return 'result'

	assert m.compute_if_absent('missing', supplier) == 'result'
	assert dict(m.items()) == {'existing': 'value', 'missing': 'result'}
	assert dict(other.items()) == {'key': 'other'}


def test_supplier_guard_is_released_after_exception():
	m = new_map()

	def supplier():
		raise ValueError('supplier failed')

	with pytest.raises(ValueError, match='supplier failed'):
		m.compute_if_absent('key', supplier)
	m['key'] = 'value'
	assert dict(m.items()) == {'key': 'value'}


def test_get_or_insert_default():
	m = new_map()
	m.get_or_insert_default('k')
	assert 'k' in m


@pytest.mark.parametrize('reuse', ['last', 'free', 'clear'])
@pytest.mark.parametrize('typ, old, default', [(u32, 100, 0), (str, 'old', '')])
def test_default_initializes_recycled_value(reuse, typ, old, default):
	m = inmem_allocate(TreeMap[str, typ])
	m['old'] = old
	if reuse == 'free':
		m['keep'] = old
	if reuse == 'clear':
		m.clear()
	else:
		del m['old']
	assert m.get_or_insert_default('new') == default
	assert m['new'] == default
	if reuse == 'free':
		assert m['keep'] == old


@allow
class DefaultValue:
	number: u32
	text: str
	fixed: Array[u32, typing.Literal[2]]
	items: DynArray[str]
	mapping: TreeMap[str, u32]
	indirect: Indirection[u32]


@pytest.mark.parametrize('reuse', ['last', 'free', 'clear'])
def test_default_initializes_recycled_compound_value(reuse):
	m = inmem_allocate(TreeMap[str, DefaultValue])
	old = m.get_or_insert_default('old')
	old.number = 100
	old.text = 'old'
	old.fixed = [3, 4]
	old.items.append('old')
	old.mapping['old'] = 100
	old.indirect.set(100)
	if reuse == 'free':
		m.get_or_insert_default('keep').number = 200
	if reuse == 'clear':
		m.clear()
	else:
		del m['old']
	value = m.get_or_insert_default('new')
	assert value.number == 0
	assert value.text == ''
	assert list(value.fixed) == [0, 0]
	assert list(value.items) == []
	assert dict(value.mapping.items()) == {}
	assert len(value.mapping) == 0
	assert value.indirect.get() == 0
	value.mapping['new'] = 5
	assert dict(value.mapping.items()) == {'new': 5}
	if reuse == 'free':
		assert m['keep'].number == 200


def test_get_or_insert_default_existing():
	m = new_map()
	m['k'] = 'val'
	result = m.get_or_insert_default('k')
	assert result == 'val'


def test_items_contains():
	m = new_map()
	m['a'] = 'x'
	m['b'] = 'y'
	assert ('a', 'x') in m.items()
	assert ('a', 'z') not in m.items()
	assert ('c', 'x') not in m.items()


def test_items_len():
	m = new_map()
	m['a'] = 'x'
	m['b'] = 'y'
	assert len(m.items()) == 2


@pytest.mark.parametrize('key', ['0', '3', '5', '9'])
def test_setitem_overwrite(key: str):
	r = {str(i): str(i) + str(i) for i in range(10)}
	m = new_map()
	m.update(r)

	m[key] = 'overwritten'
	r[key] = 'overwritten'

	same_iter(sorted(m.items()), sorted(r.items()))


def test_del_all():
	r = {str(i): str(i) for i in range(10)}
	m = new_map()
	m.update(r)

	for k in list(r.keys()):
		del m[k]
		del r[k]
		assert len(m) == len(r)

	assert len(m) == 0


def test_del_reinsert():
	m = new_map()
	m['a'] = '1'
	del m['a']
	m['a'] = '2'
	assert m['a'] == '2'
	assert len(m) == 1
