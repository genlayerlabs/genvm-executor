import pytest
from genlayer._internal import entry_calldata


def test_normalize_calldata():
	assert entry_calldata.normalize({}) == ('', [], {})
	assert entry_calldata.normalize(
		{'': 'method', 'args': [1], 'kwargs': {'key': 2}}
	) == ('method', [1], {'key': 2})


@pytest.mark.parametrize(
	'calldata',
	[
		[],
		{'': 1},
		{'args': ()},
		{'kwargs': []},
		{'kwargs': {1: 'value'}},
	],
)
def test_normalize_calldata_rejects_malformed_fields(calldata):
	with pytest.raises(TypeError, match='invalid calldata'):
		entry_calldata.normalize(calldata)
