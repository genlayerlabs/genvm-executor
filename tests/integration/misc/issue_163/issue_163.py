# { "Depends": "py-genlayer:test" }

import genlayer as gl
from genlayer.storage._internal.generate import generate_storage


@generate_storage
class Pr:
	x: gl.storage.TreeMap[str, str]


a = Pr()

try:
	a.x = {'x': 'y'}  # type: ignore
except TypeError as e:
	print(*e.args)

exit(0)
