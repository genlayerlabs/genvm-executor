#!/usr/bin/env python3

import sys
from pathlib import Path

import genlayer.calldata as calldata

sys.path.append(str(Path(__file__).parent.parent))
from fuzz_common import do_fuzzing


def calldata_decoding(buf):
	try:
		decoded = calldata.decode(buf)
	except (calldata.DecodingError, UnicodeDecodeError):
		return
	got = calldata.encode(decoded)

	assert got == buf, f'decoded is `{decoded}`'


if __name__ == '__main__':
	do_fuzzing(calldata_decoding)
