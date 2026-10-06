__all__ = (
	'ANY_METHOD',
	'Descendants',
	'ExternalAllocation',
	'InternalAllocation',
	'UseBalanceParams',
)

import dataclasses
import typing

import genlayer.calldata as calldata
from genlayer.chain import ON, ExternalMessageParams, InternalMessageParams
from genlayer.types import Address, Keccak256, u256

ANY_METHOD: typing.Final = Keccak256(b'').digest()
"""
	Call key matching any method
"""

_ROOT: typing.Final[u256] = (1 << 256) - 1


def _resolve_call_key(call_key: str | bytes | None) -> bytes:
	if call_key is None:
		return ANY_METHOD
	if isinstance(call_key, bytes):
		if len(call_key) != 32:
			raise ValueError('call_key bytes must be exactly 32 bytes')
		return call_key
	if not isinstance(call_key, str):
		raise TypeError('call_key must be str, bytes, or None')

	name = call_key.encode()
	if len(name) < 32:
		return name.ljust(32, b'\0')
	result = bytearray(Keccak256(name).digest())
	result[-1] |= 1
	return bytes(result)


def _resolve_external_call_key(call_key: bytes | None) -> bytes:
	if call_key is not None and not isinstance(call_key, bytes):
		raise TypeError('external call_key must be bytes or None')
	if isinstance(call_key, bytes):
		if len(call_key) == 4:
			return call_key + bytes(28)
		if len(call_key) != 32:
			raise ValueError('external call_key bytes must be 4 or 32 bytes')
	return _resolve_call_key(call_key)


@typing.final
@dataclasses.dataclass(frozen=True)
class InternalAllocation(calldata.CalldataEncodable):
	"""
	Allocation for an internal descendant message

	``call_key`` is a method name, a raw 32-byte key, or None for any method
	Use ``bytes(32)`` for deploys and transfers
	"""

	recipient: Address
	call_key: str | bytes | None
	fee_params: InternalMessageParams
	budget: u256
	on: ON = 'finalized'
	children: list['InternalAllocation'] = dataclasses.field(default_factory=list)

	def __to_calldata__(self):
		return {
			'recipient': self.recipient,
			'call_key': _resolve_call_key(self.call_key),
			'budget': self.budget,
			'fee_params': self.fee_params,
			'on': self.on,
		}


@typing.final
@dataclasses.dataclass(frozen=True)
class ExternalAllocation(calldata.CalldataEncodable):
	"""
	Allocation for an external descendant message

	``call_key`` is a 4-byte selector, a raw 32-byte key, or None for any method
	Use ``bytes(32)`` for empty calldata
	"""

	recipient: Address
	call_key: bytes | None
	fee_params: ExternalMessageParams
	budget: u256

	def __to_calldata__(self):
		return {
			'recipient': self.recipient,
			'call_key': _resolve_external_call_key(self.call_key),
			'budget': self.budget,
			'fee_params': self.fee_params,
		}


type Descendants = None | u256 | list[InternalAllocation | ExternalAllocation]


@typing.final
@dataclasses.dataclass(frozen=True)
class UseBalanceParams:
	fee_params: InternalMessageParams
	descendants: Descendants = None


def _flatten_allocations(roots: list[InternalAllocation | ExternalAllocation]):
	result = []
	pending = [(iter(roots), _ROOT)]
	while pending:
		nodes, parent_index = pending[-1]
		try:
			node = next(nodes)
		except StopIteration:
			pending.pop()
			continue
		if len(result) >= 1024:
			raise ValueError('descendants must contain at most 1024 allocations')
		if not isinstance(node, (InternalAllocation, ExternalAllocation)):
			raise TypeError('descendants must contain allocation objects')
		index = len(result)
		fields = node.__to_calldata__()
		fields['parent_index'] = parent_index
		result.append(fields)
		if isinstance(node, InternalAllocation):
			pending.append((iter(node.children), index))
	return result
