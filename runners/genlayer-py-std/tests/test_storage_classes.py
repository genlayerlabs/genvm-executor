from genlayer.storage._internal.generate import _known_descs, generate_storage
from genlayer.types import u32, u64


class A:
	x: u32

	def foo(self, other: u32):
		assert self.x == other


class B(A):
	y: u64

	def bar(self, other: u64):
		assert self.y == other


class C(B, A):
	pass


def test_fields():
	X = generate_storage(C)

	x = X()
	x.x = 0x01020304
	x.y = 0x05060708090A0B0C

	assert x.x == 0x01020304
	assert x.y == 0x05060708090A0B0C

	x.foo(0x01020304)
	x.bar(0x05060708090A0B0C)


def test_sizes():
	X = generate_storage(C)
	assert _known_descs[X].size == 12
