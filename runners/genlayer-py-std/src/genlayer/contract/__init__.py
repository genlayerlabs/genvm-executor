"""
Contract interaction and declaration module.

This module provides functionality for:
- Declaring main contracts with the ``Contract`` base class
- Creating type-safe interfaces with ``@interface``
- Deploying contracts with ``deploy``
- Getting contract proxies with ``get_at``
"""

__all__ = (
	'interface',
	'deploy',
	'Salted',
	'Contract',
	'get_at',
	'Proxy',
	'GenVMContractDeclaration',
	'StorageView',
	'ON',
	'UseBalanceParams',
)

import collections.abc
import json
import typing

import genlayer.calldata as calldata
from genlayer import IS_IN_VM
from genlayer.chain import IAccount
from genlayer.message_allocation import UseBalanceParams, _flatten_allocations
from genlayer.types import Address, Lazy, u256

if typing.TYPE_CHECKING or IS_IN_VM:
	import _genlayer_wasi as wasi

from genlayer._internal.on_chain.gl_call import gl_call_generic

type ON = typing.Literal['decided', 'finalized']
"""When the transaction message should be applied: ``'decided'`` or ``'finalized'``"""


def _balance_fields(
	use_balance: UseBalanceParams | None,
) -> dict[str, calldata.Encodable]:
	if use_balance is None:
		return {}
	if not isinstance(use_balance, UseBalanceParams):
		raise TypeError('use_balance must be UseBalanceParams or None')
	descendants = use_balance.descendants
	if isinstance(descendants, bool):
		raise TypeError('descendants must not be bool')
	if isinstance(descendants, (str, bytes, bytearray)):
		raise TypeError('descendants must not be str, bytes, or bytearray')
	closed = (
		descendants is None
		or (isinstance(descendants, int) and descendants == 0)
		or (isinstance(descendants, collections.abc.Sequence) and not descendants)
	)
	fields: dict[str, calldata.Encodable] = {
		'use_balance': True,
		'fee_params': use_balance.fee_params,
	}
	if not closed:
		fields['descendants'] = (
			_flatten_allocations(descendants)
			if isinstance(descendants, collections.abc.Sequence)
			else descendants
		)
	return fields


def _make_calldata_obj(method, args, kwargs) -> calldata.Encodable:
	ret = {}
	if method is not None:
		ret[''] = method
	if len(args) > 0:
		ret.update({'args': args})
	if len(kwargs) > 0:
		ret.update({'kwargs': kwargs})
	return ret


from genlayer.vm import VMError  # noqa: E402
from genlayer.vm.public_abi import StorageView  # noqa: E402


class _ContractAtViewMethod:
	__slots__ = ('_addr', '_catch_vm_error', '_name', '_state')

	def __init__(
		self,
		name: str,
		addr: Address,
		state: StorageView,
		catch_vm_error: bool = False,
	):
		self._addr = addr
		self._name = name
		self._state = state
		self._catch_vm_error = catch_vm_error

	def __call__(self, *args, **kwargs) -> calldata.Decoded | VMError:
		return self.lazy(*args, **kwargs).get()

	def lazy(self, *args, **kwargs) -> Lazy[calldata.Decoded | VMError]:
		from genlayer.vm import (
			_decode_sub_vm_result,
			_decode_sub_vm_result_catching_vm_error,
		)

		decoder = (
			_decode_sub_vm_result_catching_vm_error
			if self._catch_vm_error
			else _decode_sub_vm_result
		)

		return gl_call_generic(
			{
				'CallContract': {
					'address': self._addr,
					'calldata': _make_calldata_obj(self._name, args, kwargs),
					'storage_view': self._state.value,
					'catch_vm_error': self._catch_vm_error,
				}
			},
			decoder,
		)


class _ContractAtEmitMethod:
	__slots__ = (
		'_addr',
		'_name',
		'_on',
		'_use_balance',
		'_value',
	)

	def __init__(
		self,
		name: str | None,
		addr: Address,
		value: u256,
		on: str,
		use_balance: UseBalanceParams | None = None,
	):
		self._addr = addr
		self._name = name
		self._value = value
		self._on = on
		self._use_balance = use_balance

	def __call__(self, *args, **kwargs) -> None:
		message: dict[str, calldata.Encodable] = {
			'address': self._addr,
			'calldata': _make_calldata_obj(self._name, args, kwargs),
			'value': self._value,
			'on': self._on,
		}
		message.update(_balance_fields(self._use_balance))
		wasi.gl_call(calldata.encode({'EmitInternalMessage': message}))


@typing.runtime_checkable
class Proxy[TView, TSend](IAccount, typing.Protocol):
	"""
	Generic proxy interface for interacting with deployed GenVM contracts.

	This protocol defines the interface for contract proxies that provide type-safe
	access to view methods and write operations on deployed contracts.

	:param TView: Type representing available view methods
	:param TSend: Type representing available write methods
	"""

	@typing.overload
	def view(
		self,
		*,
		state: StorageView = StorageView.LATEST_DECIDED,
		catch_vm_error: typing.Literal[False] = False,
	) -> TView: ...

	@typing.overload
	def view(
		self,
		*,
		state: StorageView = StorageView.LATEST_DECIDED,
		catch_vm_error: typing.Literal[True],
	) -> '_CaughtViewMethods': ...

	@typing.overload
	def view(
		self,
		*,
		state: StorageView = StorageView.LATEST_DECIDED,
		catch_vm_error: bool,
	) -> TView | '_CaughtViewMethods': ...

	def view(
		self,
		*,
		state: StorageView = StorageView.LATEST_DECIDED,
		catch_vm_error: bool = False,
	) -> TView | '_CaughtViewMethods':
		"""
		Get a namespace for calling view methods.

		:param state: Storage state to query against
		:param catch_vm_error: return the callee's VM error instead of re-raising
			it. A fatal one is never caught.
		:returns: Object providing access to view methods
		"""
		...

	def emit(
		self,
		use_balance: UseBalanceParams | None = None,
		*,
		value: u256 = 0,
		on: ON = 'finalized',
	) -> TSend:
		"""
		Get a namespace for emitting write transactions.

		:param value: Amount of native tokens to transfer with the transaction
		:param on: When the transaction message should be emitted to consensus
		:param use_balance: Balance funding parameters, or None to use the sender's
			prefunded pool. Balance funding requires
			``can_use_balance_for_message_fees`` permission
		:returns: Object providing access to write methods

		.. warning::
			Emitting transactions, especially with value transfers on ``decided``
			may lead to undesired results. Prefer to use ``finalized`` (default)
		"""
		...

	def emit_transfer(
		self,
		value: u256,
		use_balance: UseBalanceParams | None = None,
		*,
		on: ON = 'finalized',
	) -> None:
		"""
		Emit a simple value transfer without calling any method. Receiver may catch it with
		:py:meth:`Contract.__receive__` (see :ref:`method-dispatch`), so users may need to supply non-zero gas

		:param value: Amount of native tokens to transfer
		:param on: When transaction message should be emitted to consensus
		:param use_balance: Fund the message fee from this contract's balance; see :py:meth:`emit`

		:raises ValueError: If value is zero
		"""
		...


class ErasedMethods(typing.Protocol):
	"""
	Protocol for dynamically accessed contract methods.

	This protocol allows accessing contract methods by name when the exact
	interface is not known for type checker in IDE
	"""

	def __getattr__(self, name: str) -> typing.Callable:
		"""
		Get a callable for the named method.

		:param name: Name of the method to access
		:returns: Callable that can invoke the method
		"""
		...


class _CaughtViewMethods(typing.Protocol):
	def __getattr__(
		self, name: str
	) -> typing.Callable[..., calldata.Decoded | VMError]: ...


class _ContractAt(Proxy[ErasedMethods, ErasedMethods]):
	__slots__ = ('_address',)

	def __init__(self, addr: Address):
		if not isinstance(addr, Address):
			raise TypeError('address expected')
		self._address = addr

	@property
	def address(self) -> Address:
		return self._address

	@typing.overload
	def view(
		self,
		*,
		state: StorageView = StorageView.LATEST_DECIDED,
		catch_vm_error: typing.Literal[False] = False,
	) -> ErasedMethods: ...

	@typing.overload
	def view(
		self,
		*,
		state: StorageView = StorageView.LATEST_DECIDED,
		catch_vm_error: typing.Literal[True],
	) -> _CaughtViewMethods: ...

	@typing.overload
	def view(
		self,
		*,
		state: StorageView = StorageView.LATEST_DECIDED,
		catch_vm_error: bool,
	) -> ErasedMethods | _CaughtViewMethods: ...

	def view(
		self,
		*,
		state: StorageView = StorageView.LATEST_DECIDED,
		catch_vm_error: bool = False,
	) -> ErasedMethods | _CaughtViewMethods:
		"""
		:param catch_vm_error: return the callee's VM error instead of re-raising
			it. A fatal one is never caught.
		"""
		return _ContractAtGetter(
			_ContractAtViewMethod, self._address, state, catch_vm_error
		)

	def emit(
		self,
		use_balance: UseBalanceParams | None = None,
		*,
		value: u256 = 0,
		on: ON = 'finalized',
	) -> ErasedMethods:
		return _ContractAtGetter(
			_ContractAtEmitMethod,
			self._address,
			value,
			on,
			use_balance,
		)

	def emit_transfer(
		self,
		value: u256,
		use_balance: UseBalanceParams | None = None,
		*,
		on: ON = 'finalized',
	) -> None:
		if value <= 0:
			raise ValueError('value must be greater than 0 for emit_transfer')
		_ContractAtEmitMethod(None, self._address, value, on, use_balance)()

	@property
	def balance(self) -> u256:
		return wasi.get_balance(self._address.as_bytes)


def get_at(address: Address, /) -> Proxy:
	"""
	Create a proxy object for interacting with a deployed GenVM contract.

	This function returns a contract proxy that provides runtime access to
	the methods of a deployed contract without requiring type annotations describing
	its interface.

	:param address: Address of the deployed contract
	:returns: ContractProxy object for interacting with the contract

	Example:
		>>> addr = Address('0x1234567890abcdef...')
		>>> contract = get_at(addr)
		>>> result = contract.view().some_view_method(arg1, arg2)
		>>> contract.emit(value=100).some_write_method(arg1)
	"""
	return _ContractAt(address)


_ContractAtGetter_P = typing.ParamSpec('_ContractAtGetter_P')


class _ContractAtGetter[T]:
	__slots__ = ('_args', '_ctor', '_kwargs')

	def __init__(
		self,
		ctor: typing.Callable[typing.Concatenate[str, _ContractAtGetter_P], T],
		*args: _ContractAtGetter_P.args,
		**kwargs: _ContractAtGetter_P.kwargs,
	):
		self._ctor = ctor
		self._args = args
		self._kwargs = kwargs

	def __getattr__(self, name: str) -> T:
		return self._ctor(name, *self._args, **self._kwargs)


@typing.runtime_checkable
class GenVMContractDeclaration[TView, TWrite](typing.Protocol):
	"""
	Protocol for defining contract interface declarations.

	This protocol is used with the `@gl.contract.interface` decorator to create
	type-safe interfaces for interacting with specific contract types.

	:param TView: Type containing view method declarations
	:param TWrite: Type containing write method declarations

	Example:
		>>> @gl.contract.interface
		>>> class MyContract:
		>>>     class View:
		>>>         def get_balance(self, user: Address) -> u256: ...
		>>>         def get_name(self) -> str: ...
		>>>
		>>>     class Write:
		>>>         def transfer(self, to: Address, amount: u256) -> None: ...
		>>>         def mint(self, to: Address, amount: u256) -> None: ...
	"""

	View: type[TView]
	"""
	Class containing declarations for all view (read-only) methods.

	All methods should be annotated with their expected return types.
	"""

	Write: type[TWrite]
	"""
	Class containing declarations for all write (state-modifying) methods.

	All methods must have return type annotations of either None or be omitted.
	"""


def interface[TView, TWrite](
	_declaration: GenVMContractDeclaration[TView, TWrite],
	/,
) -> typing.Callable[[Address], Proxy[TView, TWrite]]:
	# editorconfig-checker-disable
	"""
	Decorator for creating type-safe contract interfaces.

	This decorator creates a factory function that returns strongly-typed
	contract proxies, enabling IDE autocompletion and static type checking
	for contract interactions.

	:param _contr: Contract declaration class with View and Write nested classes
	:returns: Factory function that creates typed contract proxies

	Example:
		>>> @gl.contract.interface
		>>> class ERC20Contract:
		>>>     class View:
		>>>         def balance_of(self, owner: Address) -> u256: ...
		>>>         def total_supply(self) -> u256: ...
		>>>
		>>>     class Write:
		>>>         def transfer(self, to: Address, amount: u256) -> None: ...
		>>>         def approve(self, spender: Address, amount: u256) -> None: ...
		>>>
		>>> # Usage:
		>>> token = ERC20Contract(token_address)
		>>> balance = token.view().balance_of(user_address)  # Fully typed!
		>>> token.emit().transfer(recipient, amount)

	.. note::
		This decorator provides no runtime functionality - it's purely for
		type safety and developer experience. The actual contract interaction
		uses the same runtime mechanisms as `get_at`.
	"""
	# editorconfig-checker-enable
	return get_at


from genlayer.types import u256  # noqa: E402


class Salted:
	"""
	Requests a deterministic deploy address from :py:func:`deploy`

	The address is
	``keccak256(0xff ++ deployer ++ keccak256(contract_address ++ salt_nonce) ++ code_hash)[12:]``,
	where ``contract_address`` is the deploying contract and ``salt_nonce`` is 32 bytes big-endian
	"""

	salt_nonce: u256
	deployer: Address | None
	code_hash: bytes | None

	def __init__(
		self,
		salt_nonce: u256,
		*,
		deployer: Address | None = None,
		code_hash: bytes | None = None,
	):
		"""
		:param salt_nonce: Nonzero salt, unique per deploying contract
		:param deployer: Settlement-chain contract that performs the ``CREATE2``
		:param code_hash: 32-byte ``keccak256`` of the init code the deployer passes to ``CREATE2``

		Where the address comes from:

		- neither ``deployer`` nor ``code_hash``: computed from :py:data:`genlayer.message.create2`
		- both: computed from them, without a call
		- ``deployer`` only: ``deployer.predictGhostAddress(salt_nonce, contract_address)``, an EVM view call

		:raises ValueError: if ``salt_nonce`` is 0 or out of range, if ``code_hash`` is given without ``deployer``, or if ``code_hash`` is not 32 bytes
		"""
		if isinstance(salt_nonce, bool) or not isinstance(salt_nonce, int):
			raise TypeError('salt_nonce must be int')
		if not 0 < salt_nonce < 2**256:
			raise ValueError('salt_nonce must be in [1, 2**256)')
		if deployer is None and code_hash is not None:
			raise ValueError('code_hash requires deployer')
		if deployer is not None and not isinstance(deployer, Address):
			raise TypeError('deployer must be Address')
		if code_hash is not None and (
			not isinstance(code_hash, bytes) or len(code_hash) != 32
		):
			raise ValueError('code_hash must be 32 bytes')
		self.salt_nonce = salt_nonce
		self.deployer = deployer
		self.code_hash = code_hash

	def __repr__(self) -> str:
		return f'Salted({self.salt_nonce}, deployer={self.deployer!r}, code_hash={self.code_hash!r})'


@typing.overload
def deploy(
	use_balance: UseBalanceParams | None = None,
	*,
	code: bytes,
	args: collections.abc.Sequence[calldata.Encodable] = [],
	kwargs: collections.abc.Mapping[str, calldata.Encodable] = {},
	salted: None = None,
	value: u256 = 0,
	on: ON = 'finalized',
) -> None: ...


@typing.overload
def deploy(
	use_balance: UseBalanceParams | None = None,
	*,
	code: bytes,
	args: collections.abc.Sequence[calldata.Encodable] = [],
	kwargs: collections.abc.Mapping[str, calldata.Encodable] = {},
	salted: Salted,
	value: u256 = 0,
	on: ON = 'finalized',
) -> Address: ...


def deploy(
	use_balance: UseBalanceParams | None = None,
	*,
	code: bytes,
	args: collections.abc.Sequence[calldata.Encodable] = [],
	kwargs: collections.abc.Mapping[str, calldata.Encodable] = {},
	salted: Salted | None = None,
	value: u256 = 0,
	on: ON = 'finalized',
) -> Address | None:
	"""
	Deploy a new GenVM contract to the blockchain.

	This function deploys a new contract using the provided ``code`` and
	constructor arguments. The deployment address can be deterministic (with ``salted``)
	or non-deterministic.

	:param code: Source code of the contract to deploy. It can be regular Python code. See :ref:`runners-reference` for more information
	:param args: Positional arguments for the contract constructor
	:param kwargs: Keyword arguments for the contract constructor
	:param salted: Deploy at a deterministic address; see :py:class:`Salted`
	:param value: Amount of native tokens to send to the contract during deployment
	:param on: When to execute the deployment ('decided' or 'finalized')
	:param use_balance: Fund the deploy message fee from this contract's balance; see :py:meth:`Proxy.emit`
	:returns: The contract address if ``salted`` is given, None otherwise
	:raises ValueError: if ``predictGhostAddress`` returns the zero address; nothing is emitted then

	Example:
		>>> # Non-deterministic deployment
		>>> deploy(
		>>>     code=contract_source_str.encode('utf-8'),
		>>>     args=[initial_supply],
		>>>     kwargs={"name": "MyToken", "symbol": "MTK"}
		>>> )
		>>>
		>>> # Deterministic deployment
		>>> address = deploy(
		>>>     code=contract_source_zip_as_bytes,
		>>>     args=[initial_supply],
		>>>     salted=Salted(12345),
		>>>     value=1000  # Send 1000 native tokens
		>>> )
		>>> print(f'Contract deployed at: {address}')

	.. note::
		- For non-deterministic deployments, the address is
			assigned by the consensus and not returned. Considering asynchronous nature
			of GenLayer consensus the address should not be predicted
		- The contract's constructor will be called with the provided ``args`` and ``kwargs``
	"""

	import genlayer.message as _message

	address: Address | None = None
	salt_nonce: u256 = 0
	if salted is not None:
		if not isinstance(salted, Salted):
			raise TypeError('salted must be Salted or None')
		salt_nonce = salted.salt_nonce
		deployer, code_hash = salted.deployer, salted.code_hash
		if deployer is None:
			create2 = _message.create2
			deployer, code_hash = create2['deployer'], create2['code_hash']

		if code_hash is None:
			from genlayer._internal import predict_ghost_address

			address = predict_ghost_address(deployer, salt_nonce, _message.contract_address)
		else:
			from genlayer._internal import create2_address

			address = create2_address(
				deployer, code_hash, _message.contract_address, salt_nonce
			)

	message: dict[str, calldata.Encodable] = {
		'calldata': _make_calldata_obj(None, args, kwargs),
		'code': code,
		'value': value,
		'on': on,
		'salt_nonce': salt_nonce,
	}
	message.update(_balance_fields(use_balance))
	wasi.gl_call(calldata.encode({'EmitInternalDeployMessage': message}))

	return address


class Contract(IAccount):
	"""
	Class for declaring main GenVM contract.

	This class must be inherited by user contracts to be deployable on GenVM.
	It provides essential contract functionality including balance access,
	address information, and storage proxying.

	Only one ``Contract`` subclass is allowed per module. The class automatically
	generates storage management code and registers itself as the main contract.

	Example:
		>>> import genlayer as gl
		>>>
		>>> class MyContract(gl.contract.Contract):
		>>>     def __init__(self, initial_value: int):
		>>>         self.value = initial_value
		>>>
		>>>     @gl.public.view
		>>>     def get_value(self) -> int:
		>>>         return self.value
		>>>
		>>>     @gl.public.write
		>>>     def set_value(self, new_value: int):
		>>>         self.value = new_value

	.. warning::
		Only one Contract subclass is allowed per Python module. Attempting
		to define multiple Contract subclasses will raise a TypeError.
	"""

	def __init_subclass__(cls) -> None:
		"""
		Initialize the contract subclass and register it as the main contract.

		:raises TypeError: If another Contract subclass already exists in this module
		"""
		global __known_contract__
		if __known_contract__ is not None:
			raise TypeError(
				f'only one contract is allowed; first: `{__known_contract__}` second: `{cls}`'
			)

		cls.__gl_contract__ = True
		from genlayer.storage._internal.generate import generate_storage

		generate_storage(cls)
		__known_contract__ = cls

	@property
	def balance(self) -> u256:
		return wasi.get_self_balance()

	@property
	def address(self) -> Address:
		import genlayer.message as message

		return message.contract_address

	def emit_transfer(
		self,
		value: u256,
		use_balance: UseBalanceParams | None = None,
		*,
		on: ON = 'finalized',
	) -> None:
		import warnings

		warnings.warn('Emitting transfer to self without data makes little sense')
		from genlayer.contract import get_at

		get_at(self.address).emit_transfer(value, use_balance, on=on)

	def __handle_undefined_method__(
		self, method_name: str, args: list[typing.Any], kwargs: dict[str, typing.Any]
	):
		"""
		Handle a call without value to a method the contract does not define.

		It also receives a plain transfer without value if the contract does not define
		:py:meth:`__receive__`, with ``method_name`` set to ``''``. Defaults to :py:meth:`__handle_undefined_method_payable__`. Like every special
		method it is called by the VM only, so it must not be decorated with ``gl.public``.
		See :ref:`method-dispatch`

		:param method_name: Name of the method that was called
		:param args: Positional arguments passed to the method
		:param kwargs: Keyword arguments passed to the method

		Example:
			>>> class MyContract(gl.contract.Contract):
			>>>     def __handle_undefined_method__(self, method_name: str, args: list, kwargs: dict):
			>>>         if method_name == "fallback_method":
			>>>             self.handle_fallback(args, kwargs)
			>>>         else:
			>>>             raise ValueError(f"Unknown method: {method_name}")
		"""
		return self.__handle_undefined_method_payable__(method_name, args, kwargs)

	def __handle_undefined_method_payable__(
		self, method_name: str, args: list[typing.Any], kwargs: dict[str, typing.Any]
	):
		"""
		Handle a call to a method the contract does not define, with or without value.

		Without an override, a call with value to an undefined method is refused.
		It also receives a plain transfer if the contract does not define
		:py:meth:`__receive__`, with ``method_name`` set to ``''``.
		See :ref:`method-dispatch`

		:param method_name: Name of the method that was called
		:param args: Positional arguments passed to the method
		:param kwargs: Keyword arguments passed to the method
		"""
		raise NotImplementedError()

	def __receive__(self):
		"""
		Handle a plain value transfer to this contract.

		It is always payable and must not be decorated with ``gl.public``.
		See :ref:`method-dispatch`

		Example:
			>>> class MyContract(gl.contract.Contract):
			>>>     def __receive__(self):
			>>>         self.total_received += gl.message.value
		"""
		raise NotImplementedError()

	@classmethod
	def __get_schema__(cls) -> str:
		"""
		Generate and return the JSON schema for this contract.

		This method analyzes the contract class and generates a JSON schema
		describing its public interface, including all public methods and
		their type signatures.

		:returns: JSON string containing the contract schema

		.. note::
			This method is used internally by the GenVM runtime for
			contract introspection and interface generation
		"""
		import genlayer._internal.get_schema as _get_schema

		res = _get_schema.get_schema(cls)
		return json.dumps(res, separators=(',', ':'))


Contract.__handle_undefined_method__.__isabstractmethod__ = True  # type: ignore
Contract.__handle_undefined_method_payable__.__isabstractmethod__ = True  # type: ignore
Contract.__receive__.__isabstractmethod__ = True  # type: ignore

__known_contract__: type[Contract] | None = None
