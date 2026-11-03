import datetime
import time

import pytest
from genlayer.storage import TreeMap
from genlayer.storage._internal.generate import generate_storage


@generate_storage
class Store:
	dt: datetime.datetime


class ContextDependentTimezone(datetime.tzinfo):
	def utcoffset(self, dt: datetime.datetime | None) -> datetime.timedelta | None:
		if dt is None:
			return None
		return datetime.timedelta(hours=3)

	def dst(self, dt: datetime.datetime | None) -> datetime.timedelta:
		return datetime.timedelta()


class InvalidTimezone(datetime.tzinfo):
	def utcoffset(self, dt: datetime.datetime | None) -> None:
		return None


@pytest.mark.parametrize(
	'expr',
	[
		datetime.datetime.now(),
		datetime.datetime.now().astimezone(datetime.timezone.utc),
		datetime.datetime.now().astimezone(datetime.timezone(datetime.timedelta(hours=4))),
		datetime.datetime.now().astimezone(datetime.timezone(datetime.timedelta(hours=2))),
		datetime.datetime.now().astimezone(datetime.timezone(datetime.timedelta(hours=-4))),
		datetime.datetime.now().astimezone(
			datetime.timezone(datetime.timedelta(hours=-11))
		),
		datetime.datetime.now().astimezone(datetime.timezone(datetime.timedelta(hours=11))),
		datetime.datetime.fromisoformat('2024-11-26T06:42:42.424242Z'),
	],
)
def test_dt(expr: datetime.datetime):
	st = Store()
	st.dt = expr
	assert expr == st.dt


def test_dt_uses_value_dependent_utc_offset():
	expr = datetime.datetime(2025, 1, 2, 3, 4, 5, tzinfo=ContextDependentTimezone())
	st = Store()
	st.dt = expr
	assert expr == st.dt


def test_invalid_utc_offset_does_not_modify_value():
	initial = datetime.datetime(2025, 1, 2, tzinfo=datetime.UTC)
	st = Store()
	st.dt = initial

	with pytest.raises(ValueError, match=r'utcoffset\(\) returned None'):
		st.dt = datetime.datetime(2026, 2, 3, tzinfo=InvalidTimezone())

	assert st.dt == initial


UTC = datetime.UTC


@pytest.fixture
def local_time_is_utc(monkeypatch):
	# naive datetimes are stored as local time, make the tests independent of the machine
	monkeypatch.setenv('TZ', 'UTC')
	time.tzset()
	yield
	monkeypatch.undo()
	time.tzset()


@pytest.mark.parametrize(
	'val',
	[
		pytest.param(datetime.datetime.max.replace(tzinfo=UTC), id='max_utc'),
		pytest.param(datetime.datetime.min.replace(tzinfo=UTC), id='min_utc'),
		pytest.param(
			datetime.datetime(2600, 1, 1, 0, 0, 0, 999999, UTC), id='2600_999999us'
		),
		pytest.param(
			datetime.datetime(9999, 12, 31, 23, 59, 59, 999999, UTC), id='max_no_float'
		),
		pytest.param(
			datetime.datetime(2514, 6, 1, 12, 0, 0, 999999, UTC), id='2514_999999us'
		),
		pytest.param(datetime.datetime(1970, 1, 1, tzinfo=UTC), id='epoch'),
		pytest.param(
			datetime.datetime(1969, 12, 31, 23, 59, 59, tzinfo=UTC), id='before_epoch'
		),
		pytest.param(
			datetime.datetime(1969, 12, 31, 23, 59, 59, 500000, tzinfo=UTC),
			id='last_second_before_epoch',
		),
		pytest.param(
			datetime.datetime(
				1900, 6, 15, 12, 0, 0, 250000, datetime.timezone(datetime.timedelta(hours=3))
			),
			id='1900_plus3',
		),
		pytest.param(
			datetime.datetime(
				1, 1, 2, 3, 4, 5, 6, datetime.timezone(datetime.timedelta(hours=-5))
			),
			id='year_1',
		),
		pytest.param(
			datetime.datetime(
				2024,
				1,
				1,
				0,
				0,
				0,
				123456,
				datetime.timezone(datetime.timedelta(seconds=3600, microseconds=500)),
			),
			id='offset_with_microseconds',
		),
	],
)
def test_dt_roundtrip_is_exact(val: datetime.datetime):
	st = Store()
	st.dt = val
	got = st.dt
	assert got == val
	assert got.microsecond == val.microsecond
	assert got.utcoffset() == val.utcoffset()


@pytest.mark.parametrize(
	'val',
	[
		datetime.datetime.max,
		datetime.datetime(2600, 1, 1, 0, 0, 0, 999999),
		datetime.datetime(1969, 12, 31, 23, 59, 59),
		datetime.datetime(1969, 12, 31, 23, 59, 59, 500000),
		datetime.datetime(1900, 6, 15, 12, 0, 0, 250000),
		datetime.datetime(1, 1, 2),
	],
)
def test_naive_dt_roundtrip_is_exact(local_time_is_utc, val: datetime.datetime):
	st = Store()
	st.dt = val
	got = st.dt
	assert got == val
	assert got.tzinfo is None


def test_dt_max_stays_readable_and_does_not_leave_year_10000():
	st = Store()
	st.dt = datetime.datetime.max.replace(tzinfo=UTC)
	assert st.dt == datetime.datetime.max.replace(tzinfo=UTC)


def test_last_second_before_epoch_is_not_moved_to_the_epoch():
	st = Store()
	st.dt = datetime.datetime(1969, 12, 31, 23, 59, 59, 500000, tzinfo=UTC)
	# it used to come back as 1970-01-01 00:00:00.5, one second later
	assert st.dt != datetime.datetime(1970, 1, 1, 0, 0, 0, 500000, tzinfo=UTC)


def _stored_seconds(st: Store) -> bytes:
	return bytes(st._storage_slot.read(st._off, 8))


def test_non_negative_seconds_keep_the_bytes_they_always_had():
	# the seconds field became signed: for every value that could be stored before
	# it is still the same 8 little-endian bytes, so existing state keeps being read
	val = datetime.datetime(2024, 6, 15, 12, 0, 0, 123456, UTC)
	st = Store()
	st.dt = val
	assert _stored_seconds(st) == int(val.timestamp()).to_bytes(8, 'little')


def test_existing_state_is_read_as_before():
	from genlayer.storage._internal.generate import _dt_desc

	st = Store()
	raw = _dt_desc.get(st._storage_slot, st._off)
	# what the previous implementation wrote for 2023-11-14 22:13:20.000123 +03:00
	raw.seconds = 1700000000
	raw.micros = 123
	raw.has_tz = True
	raw.off_days = 0
	raw.off_seconds = 3 * 3600
	raw.off_micros = 0

	assert st.dt == datetime.datetime(
		2023, 11, 15, 1, 13, 20, 123, datetime.timezone(datetime.timedelta(hours=3))
	)


@generate_storage
class Pr:
	x: TreeMap[str, str]


a = Pr()
a.x.update({'x': 'y'})
