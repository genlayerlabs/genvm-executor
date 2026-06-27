# { "Depends": "py-genlayer:test" }

from dataclasses import dataclass

import genlayer as gl
from genlayer.storage import allow


@allow
@dataclass
class Test[T]:
	foo: T


tst = gl.storage.inmem_allocate(Test[str], '123')
print(tst)

exit(0)
