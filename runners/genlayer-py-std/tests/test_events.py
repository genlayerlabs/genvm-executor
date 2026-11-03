import pytest
from genlayer.chain import Event


def test_event_requires_init():
	with pytest.raises(TypeError, match='must define __init__'):

		class MissingInit(Event):
			pass


def test_invalid_sig():
	with pytest.raises(TypeError):

		class KwPosEv(Event):
			def __init__(self, a): ...

	with pytest.raises(TypeError):

		class VarArgEv(Event):
			def __init__(self, *a): ...


def test_sorts():
	class Sorts(Event):
		def __init__(self, b, a, /): ...

	assert Sorts.indexed == ('a', 'b')
	assert Sorts.signature == 'Sorts(a,b)'

	Sorts(1, 2)


def test_with_blob():
	class Sorts(Event):
		def __init__(self, b, a, /, **blob): ...

	assert Sorts.indexed == ('a', 'b')
	assert Sorts.signature == 'Sorts(a,b)'

	Sorts(1, 2, gg=5)


def test_name_override():
	class Name1(Event):
		name = 'Name2'

		def __init__(self, b, a, /, **blob): ...

	assert Name1.name == 'Name2'
	assert Name1.signature == 'Name2(a,b)'


def _capture_emitted(monkeypatch):
	emitted = []
	monkeypatch.setattr(
		Event,
		'emit_raw',
		staticmethod(lambda topics, blob: emitted.append((topics, blob))),
	)
	return emitted


def _as_topic(value):
	from genlayer import calldata

	enc = calldata.encode(value)
	assert len(enc) <= 32
	return enc + b'\x00' * (32 - len(enc))


def test_positional_args_follow_declaration_order(monkeypatch):
	emitted = _capture_emitted(monkeypatch)

	class Transfer(Event):
		def __init__(self, to: str, amount: int, /): ...

	# declared as (to, amount), sorted as (amount, to): the values must still
	# end up under the names they were declared with
	Transfer('alice', 100).emit()

	topics, blob = emitted[-1]
	assert blob == {'to': 'alice', 'amount': 100}
	assert Transfer.indexed == ('amount', 'to')
	assert Transfer.signature == 'Transfer(amount,to)'
	# topics keep the sorted order, each carrying the value of its own field
	assert topics[1:] == [_as_topic(100), _as_topic('alice')]


def test_positional_args_with_blob_follow_declaration_order(monkeypatch):
	emitted = _capture_emitted(monkeypatch)

	class Action(Event):
		def __init__(self, user_id: int, action: str, /, **blob): ...

	Action(42, 'create', timestamp=1234567890).emit()

	topics, blob = emitted[-1]
	assert blob == {'user_id': 42, 'action': 'create', 'timestamp': 1234567890}
	assert topics[1:] == [_as_topic('create'), _as_topic(42)]


def test_declaration_order_equal_to_sorted_order_is_unchanged(monkeypatch):
	emitted = _capture_emitted(monkeypatch)

	class Sorted(Event):
		def __init__(self, a: int, b: int, /): ...

	Sorted(1, 2).emit()

	_, blob = emitted[-1]
	assert blob == {'a': 1, 'b': 2}


def test_positional_count_mismatch_reports_declared_order():
	class Pair(Event):
		def __init__(self, b, a, /): ...

	with pytest.raises(TypeError, match=r"expected \('b', 'a'\)"):
		Pair(1)


def test_indexed_field_in_blob_is_rejected():
	class Dup(Event):
		def __init__(self, b, a, /, **blob): ...

	with pytest.raises(TypeError, match='must not be present in blob'):
		Dup(1, 2, a=3)
