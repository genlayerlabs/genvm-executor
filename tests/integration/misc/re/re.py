# { "Depends": "py-genlayer:test" }

import re


def m(pat: str, st: str):
	val = re.match(pat, st)
	assert val is not None, f'Failed to match {st} with {pat}'
	return val.span()


assert m('(ab|ba)', 'ab') == (0, 2)
assert m('(ab|ba)', 'ba') == (0, 2)
assert m('(abc|bac|ca|cb)', 'abc') == (0, 3)
assert m('(abc|bac|ca|cb)', 'bac') == (0, 3)
assert m('(abc|bac|ca|cb)', 'ca') == (0, 2)
assert m('(abc|bac|ca|cb)', 'cb') == (0, 2)
assert m('((a)|(b)|(c))', 'a') == (0, 1)
assert m('((a)|(b)|(c))', 'b') == (0, 1)
assert m('((a)|(b)|(c))', 'c') == (0, 1)

exit(0)
