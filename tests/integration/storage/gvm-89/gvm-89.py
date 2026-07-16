# { "Depends": "py-genlayer:test" }

from dataclasses import dataclass

import genlayer as gl
from genlayer.storage import allow


@allow
@dataclass
class Foo:
	x: gl.storage.DynArray[str]


class Main(gl.contract.Contract):
	f: gl.storage.DynArray[Foo]

	@gl.public.write
	def main(self):
		self.f.append(Foo(['123']))
		return [i for i in self.f]
