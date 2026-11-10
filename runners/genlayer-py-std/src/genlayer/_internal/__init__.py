__all__ = ('create2_address', 'predict_ghost_address')

import os
import typing

from genlayer.types import Address, u256
from genlayer.types.keccak import Keccak256

from ..types import Lazy


class LazyApi[T, **R](typing.Protocol):
	def __call__(self, *args: R.args, **kwargs: R.kwargs) -> T:
		"""
		Immediately execute and get the result
		"""
		...

	def lazy(self, *args: R.args, **kwargs: R.kwargs) -> Lazy[T]:
		"""
		Wrap evaluation into ``Lazy`` and return it
		"""
		...


def _lazy_api[T, **R](fn: typing.Callable[R, Lazy[T]]) -> LazyApi[T, R]:
	"""
	Reuses a function for better docs
	"""

	def eager(*args: R.args, **kwargs: R.kwargs) -> T:
		return fn(*args, **kwargs).get()

	if os.getenv('GENERATING_DOCS', 'false') == 'true':
		annots: dict = dict(fn.__annotations__)
		annots['return'] = annots['return'].__args__[0]
		eager.__annotations__ = annots
		eager.__module__ = fn.__module__
		import inspect
		import textwrap

		eager.__signature__ = inspect.signature(fn)  # pyright: ignore[reportFunctionMemberAccess]
		eager.__doc__ = (
			textwrap.dedent(fn.__doc__ or '')
			+ '\n\n.. note::\n\tsupports ``.lazy()`` version, which will return :py:class:`~genlayer.types.Lazy`'
		)
	eager.__name__ = fn.__name__
	eager.lazy = fn  # pyright: ignore[reportFunctionMemberAccess]
	return typing.cast(LazyApi[T, R], eager)


def create2_address(
	deployer: Address, code_hash: bytes, parent: Address, salt_nonce: u256, /
) -> Address:
	hasher = Keccak256(b'\xff')
	hasher.update(deployer.as_bytes)
	hasher.update(
		Keccak256(parent.as_bytes + salt_nonce.to_bytes(32, 'big', signed=False)).digest()
	)
	hasher.update(code_hash)
	return Address(hasher.digest()[12:])


def predict_ghost_address(
	deployer: Address, salt_nonce: u256, sender: Address, /
) -> Address:
	import genlayer.evm as evm

	@evm.contract_interface
	class GhostFactory:
		class View:
			def predictGhostAddress(
				self, salt_nonce: u256, sender: Address, /
			) -> Address: ...

		class Write:
			pass

	address = GhostFactory(deployer).view().predictGhostAddress(salt_nonce, sender)
	if address == Address(bytes(20)):
		raise ValueError('predictGhostAddress returned the zero address')
	return address
