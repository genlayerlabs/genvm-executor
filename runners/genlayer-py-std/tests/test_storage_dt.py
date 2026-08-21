import datetime

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


@generate_storage
class Pr:
	x: TreeMap[str, str]


a = Pr()
a.x.update({'x': 'y'})
