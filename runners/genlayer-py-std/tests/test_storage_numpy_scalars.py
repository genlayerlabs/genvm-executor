"""
A numpy scalar storage field must always occupy exactly the size of its declared
dtype: a value of another dtype is rejected and never changes the neighbouring
fields.
"""

import decimal
import fractions

import numpy as np
import pytest
from genlayer.storage import DynArray
from genlayer.storage._internal.generate import generate_storage
from genlayer.types import u32


@generate_storage
class Player:
	level: np.uint8
	coins: u32
	ratio: np.float32
	delta: np.int64


@generate_storage
class Board:
	scores: DynArray[np.float32]


@generate_storage
class Counter:
	total: np.uint32


def new_player() -> Player:
	p = Player()
	p.level = np.uint8(7)
	p.coins = 1_000_000
	p.ratio = np.float32(1.5)
	p.delta = np.int64(-9)
	return p


def assert_untouched(p: Player) -> None:
	assert p.level == 7
	assert p.coins == 1_000_000
	assert p.ratio == np.float32(1.5)
	assert p.delta == -9


def test_same_dtype_roundtrip():
	p = new_player()
	p.level = np.uint8(200)
	p.ratio = np.float32(-0.25)
	p.delta = np.int64(2**40)

	assert p.level == 200 and type(p.level) is np.uint8
	assert p.ratio == np.float32(-0.25) and type(p.ratio) is np.float32
	assert p.delta == 2**40 and type(p.delta) is np.int64
	assert p.coins == 1_000_000


@pytest.mark.parametrize(
	'field, value',
	[
		('level', np.int64(3)),  # wider, what numpy integer math produces
		('level', np.uint16(3)),  # wider, same kind
		('ratio', np.float64(2.0)),  # what ``np.float32 * float`` promotes to
		('delta', np.int8(3)),  # narrower
		('delta', np.uint64(3)),  # same width, different signedness
		('level', np.float32(3.0)),  # different kind
		('ratio', np.int32(3)),  # same width, different kind
	],
)
def test_numpy_scalar_of_another_dtype_is_rejected_and_nothing_is_overwritten(
	field: str, value: np.generic
):
	p = new_player()

	with pytest.raises(TypeError, match='expected a value of dtype'):
		setattr(p, field, value)

	assert_untouched(p)


def test_rejection_message_names_both_dtypes_and_the_fix():
	p = new_player()

	with pytest.raises(TypeError, match=r'dtype uint8, got int64.*uint8\(value\)'):
		p.level = np.int64(3)


def test_explicit_conversion_is_accepted():
	p = new_player()
	p.level = np.uint8(np.int64(3))
	p.ratio = np.float32(np.float64(2.0))

	assert p.level == 3 and p.ratio == np.float32(2.0)
	assert p.coins == 1_000_000 and p.delta == -9


def test_array_element_of_another_dtype_does_not_spill_into_neighbours():
	b = Board()
	b.scores.extend([np.float32(1.5), np.float32(2.5), np.float32(3.5)])

	with pytest.raises(TypeError):
		b.scores[0] = np.float32(1.0) * np.float64(2.0)

	assert [float(x) for x in b.scores] == [1.5, 2.5, 3.5]


def test_array_append_of_another_dtype_is_rejected():
	b = Board()
	b.scores.append(np.float32(1.5))

	with pytest.raises(TypeError):
		b.scores.append(np.float64(2.0))

	assert len(b.scores) == 1
	assert b.scores[0] == np.float32(1.5)


def test_narrower_value_does_not_leave_old_high_bytes():
	c = Counter()
	c.total = np.uint32(70000)

	with pytest.raises(TypeError):
		c.total = np.uint8(5)

	assert c.total == 70000


@pytest.mark.parametrize('value', [0, 3, 255])
def test_python_int_in_range_is_accepted(value: int):
	p = new_player()
	p.level = value

	assert p.level == value and type(p.level) is np.uint8
	assert p.coins == 1_000_000 and p.delta == -9


@pytest.mark.parametrize('value', [256, 300, -1])
def test_python_int_out_of_range_is_rejected(value: int):
	p = new_player()

	with pytest.raises(OverflowError):
		p.level = value

	assert_untouched(p)


def test_python_float_into_float_field_is_accepted():
	p = new_player()
	p.ratio = 2.5

	assert p.ratio == np.float32(2.5) and type(p.ratio) is np.float32
	assert p.level == 7 and p.coins == 1_000_000


def test_python_int_into_float_field_is_accepted():
	p = new_player()
	p.ratio = 2

	assert p.ratio == np.float32(2.0)
	assert p.level == 7 and p.coins == 1_000_000


@pytest.mark.parametrize('field', ['level', 'delta'])
@pytest.mark.parametrize('value', [2.5, 3.0])
def test_python_float_into_integer_field_is_rejected_instead_of_truncated(
	field: str, value: float
):
	p = new_player()

	with pytest.raises(TypeError, match='expected an integer'):
		setattr(p, field, value)

	assert_untouched(p)


@pytest.mark.parametrize('field', ['level', 'ratio'])
@pytest.mark.parametrize(
	'value',
	['5', b'1', fractions.Fraction(5, 2), decimal.Decimal('2.5'), True, np.True_, None],
)
def test_non_number_is_rejected_instead_of_converted(field: str, value):
	p = new_player()

	with pytest.raises(TypeError, match='expected'):
		setattr(p, field, value)

	assert_untouched(p)
