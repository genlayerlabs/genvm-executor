# { "Depends": "py-genlayer:test" }
import math
import struct

import genlayer as gl


# `repr()` rounds; the raw bits are what a divergent FPU actually differs in.
def _bits(value: float) -> bytes:
	return struct.pack('<d', value)


class Contract(gl.contract.Contract):
	def __init__(self):
		res: list[bytes] = []

		# Transcendental and irrational results are the classic place where a
		# hardware FPU's last bit — or an x87 80-bit intermediate — leaks
		# per-machine nondeterminism. In det mode these go through softfloat.
		acc = 0.0
		for i in range(1, 50):
			x = float(i) * 0.123456789
			acc += math.sin(x) * math.cos(x)
			acc += math.exp(x % 3.0)
			acc += math.sqrt(x) * math.log(x + 1.0)
			acc += math.atan2(x, 1.0 + x) ** 1.5
		res.append(_bits(acc))

		for value in [math.pi, math.e, 2.0**0.5, 0.1 + 0.2, -0.0]:
			res.append(_bits(value))

		# Overflow, and the two ways to reach a NaN whose payload is unspecified
		# by IEEE-754 and therefore free to differ between implementations.
		res.append(_bits(1e308 * 10))
		res.append(_bits(float('nan')))
		res.append(_bits(math.inf - math.inf))

		gl.vm.UserError.immediate(res)
