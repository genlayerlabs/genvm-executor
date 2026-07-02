# { "Depends": "py-genlayer:test" }

import genlayer as gl
from genlayer.storage._internal.generate import generate_storage
from genlayer.types import u32


@generate_storage
class UserStorage:
	m: gl.storage.TreeMap[str, u32]


tst = UserStorage()

tst.m['1'] = 12
tst.m['2'] = 13
del tst.m['1']
print('1' in tst.m, tst.m['2'])

exit(0)
