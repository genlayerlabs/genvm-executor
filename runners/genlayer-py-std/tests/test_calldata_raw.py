import genlayer.calldata as calldata


def test_raw_splices_verbatim():
	inner = calldata.encode({'a': 1})
	assert calldata.encode(calldata.Raw(inner)) == inner


def test_raw_inside_a_container():
	assert calldata.decode(calldata.encode([calldata.Raw(calldata.encode(7))])) == [7]


def test_raw_of_a_bytes_encoding_is_not_a_double_wrap():
	assert calldata.decode(calldata.encode(calldata.Raw(calldata.encode(b'ab')))) == b'ab'
