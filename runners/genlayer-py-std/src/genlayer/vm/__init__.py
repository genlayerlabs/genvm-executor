"""
Virtual Machine execution and sandbox module.

This module provides:
- Sandbox execution with ``spawn_sandbox`` and ``spawn_runner``
- Non-deterministic execution with ``run_nondet_default`` and ``run_nondet``
- Result types: ``Return``, ``VMError``, ``UserError``, ``Result``
- Event emission with ``Event``
"""

__all__ = (
	# vm
	'spawn_runner',
	'spawn_sandbox',
	'run_nondet',
	'run_nondet_default',
	'unpack_result',
	'Return',
	'VMError',
	'UserError',
	'Result',
	'trace',
	'trace_time_micro',
	'yield_',
	'get_timestamp',
	'register_runner',
	'map_file',
	'ABI',
	'SandboxChangesOnError',
	'RunnerID',
)

import typing

from genlayer import IS_IN_VM

if typing.TYPE_CHECKING or IS_IN_VM:
	import _genlayer_wasi as wasi

IS_INSIDE = IS_IN_VM

# ruff: noqa: E402

import collections.abc
import dataclasses
import datetime
import typing

import genlayer as gl
import genlayer._internal.on_chain.gl_call as gl_call
import genlayer.calldata as calldata
from genlayer._internal import _lazy_api
from genlayer.types import Address, Lazy

from . import public_abi as ABI
from .public_abi import ResultCode

RunnerID = typing.NewType('RunnerID', str)
"""
Id of a runner: ``name:hash``, ``custom:<hash>``, ``chain:<address>``, or
``contract`` for this contract's own one
"""


class RunnerIDOps:
	CONTRACT = RunnerID('contract')

	@typing.overload
	@staticmethod
	def new_chain(
		addr: Address, state: typing.Literal['d', 'f'] | None = None, slot_id: None = None
	) -> RunnerID: ...

	@typing.overload
	@staticmethod
	def new_chain(
		addr: Address, state: typing.Literal['d', 'f'], slot_id: bytes
	) -> RunnerID: ...

	@staticmethod
	def new_chain(
		addr: Address,
		state: typing.Literal['d', 'f'] | None = None,
		slot_id: bytes | None = None,
	) -> RunnerID:
		"""
		Creates a new runner id for the given contract address.

		:param addr: contract address in hex
		:return: runner id
		"""

		components = ['chain', addr.as_hex]

		if state is not None:
			if state not in ('d', 'f'):
				raise ValueError(f'state must be "d" or "f", got {state}')
			components.append(state)

		if slot_id is not None:
			if state is None:
				raise TypeError('state must be provided if slot_id is provided')
			if len(slot_id) != 32:
				raise ValueError(f'slot_id must be 32 bytes, got {len(slot_id)} bytes')
			components.append(gl.gvm32.encode(slot_id))

		return RunnerID(':'.join(components))


@dataclasses.dataclass
class Return[T: calldata.Decoded]:
	"""
	Represents a successful return value from a VM operation.
	"""

	calldata: T
	"""
	Decoded return value from the VM execution
	"""


@dataclasses.dataclass
class VMError:
	"""
	Represents an error that occurred within the VM during execution.

	It indicates user-caused error, such as OOM.
	"""

	message: str
	"""
	Description of the VM error that occurred. It begins with code, such as ``exit_code``
	"""

	public_code: str
	"""
	Extracted public code from the full message, which is the part before the first `` # `` detail suffix.
	"""

	detail: str
	"""
	Additional detail about the error, which is the part after the first `` # `` suffix.
	"""

	def __init__(self, message: str, /):
		self.message = message
		self.public_code, _, self.detail = message.partition(' # ')

	def __str__(self) -> str:
		if self.detail:
			return f'VMError("{self.public_code} # {self.detail}")'
		return f'VMError("{self.public_code}")'


class UserError(Exception):
	"""
	Represents an error that user contract rose during execution of their code in the VM.
	"""

	data: calldata.Decoded
	"""
	User-provided message. Be careful to use concise message, as by default they are checked for strict equality
	by the validator
	"""

	def __init__(self, data: calldata.Decoded, /):
		super().__init__()
		self.data = data

	def __str__(self) -> str:
		return 'UserError(' + repr(self.data) + ')'

	def __eq__(self, other) -> bool:
		if not isinstance(other, UserError):
			return False
		return self.data == other.data

	def __hash__(self) -> int:
		return hash(self.data)

	@staticmethod
	def immediate(reason: calldata.Encodable) -> typing.NoReturn:
		"""
		Performs an immediate error, current VM won't be able to handle it, stack unwind will not happen
		"""

		gl_call.user_error(reason)


type Result[T: calldata.Decoded] = Return[T] | VMError | UserError
"""
Union type representing all possible outcomes from a VM operation.
"""


def _decode_sub_vm_result_retn(
	data: collections.abc.Buffer,
) -> Result:
	mem = memoryview(data)
	if mem[0] == ResultCode.USER_ERROR:
		return UserError(calldata.decode(mem[1:]))
	if mem[0] == ResultCode.RETURN:
		return Return(calldata.decode(mem[1:]))
	if mem[0] == ResultCode.VM_ERROR:
		return VMError(str(mem[1:], encoding='utf8'))
	raise ValueError(f'unknown result code {mem[0]}')


def unpack_result[T: calldata.Decoded](res: Result[T], /) -> T:
	"""
	Extracts the successful result from a VM operation result.

	:param res: The result from a VM operation
	:return: The actual return value if successful
	:raises UserError: If the result represents a user error
	:raises UserError: If the result represents a ``VMError`` (rewrapped as user error)

	Example:
		>>> result = gl.vm.spawn_sandbox(lambda: 42)
		>>> value = unpack_result(result)  # Returns 42 or re-raises on error
	"""
	if isinstance(res, UserError):
		raise res
	if isinstance(res, VMError):
		raise UserError('vm error: ' + res.message)
	return res.calldata


def _decode_sub_vm_result(
	data: collections.abc.Buffer,
) -> calldata.Decoded:
	return unpack_result(_decode_sub_vm_result_retn(data))


type SandboxChangesOnError = typing.Literal['inherit']
"""
Defines what happens to storage changes and emissions on non-return result of a sandbox
"""


@_lazy_api
def spawn_runner(
	runner: RunnerID,
	data: collections.abc.Buffer,
	/,
	*,
	allow_write_storage: bool = False,
	allow_send_messages: bool = False,
	custom_runners: list[RunnerID] | None = None,
	changes_on_error: SandboxChangesOnError = 'inherit',
) -> Lazy[Return[calldata.Decoded] | VMError | UserError]:
	"""
	Runs another runner in an isolated sub-VM, handing it ``data`` verbatim.

	This is the general form of :py:func:`spawn_sandbox`: the child is whatever
	``runner`` names, so it need not be Python, and ``data`` is its entry payload
	rather than a pickled callable. Determinism of the spawned VM matches the
	determinism of the current VM.

	Each ``allow_*`` flag grants the corresponding permission, but only if the
	current VM holds it as well.

	:param runner: runner id to execute: a ``custom:<hash>``/``name:hash``/``chain:`` id, or ``contract`` for this contract's own runner
	:param data: entry payload, passed to the child untouched
	:param allow_write_storage: Whether to allow storage writes in the child
	:param allow_send_messages: Whether to allow sending messages in the child
	:param custom_runners: ``custom:<hash>`` ids visible to the child; ``None`` grants this VM's entire set, a list grants exactly that subset of it
	:param changes_on_error: see :py:obj:`SandboxChangesOnError`

	Both sides have to agree on what ``data`` means. Use
	:py:mod:`genlayer.calldata` for it unless the runner documents otherwise:

	Example:
		>>> answer = spawn_runner(rid, calldata.encode(30))
		>>> calldata.decode(unpack_result(answer))
	"""
	return gl_call.gl_call_generic(
		{
			'Sandbox': {
				'data': data,
				'runner': runner,
				'allow_write_storage': allow_write_storage,
				'allow_send_messages': allow_send_messages,
				'custom_runners': custom_runners,
				'changes_on_error': changes_on_error,
			}
		},
		_decode_sub_vm_result_retn,
	)


@_lazy_api
def spawn_sandbox[T: calldata.Decoded](
	fn: typing.Callable[[], T],
	*,
	allow_write_storage: bool = False,
	allow_send_messages: bool = False,
	custom_runners: list[RunnerID] | None = None,
	changes_on_error: SandboxChangesOnError = 'inherit',
) -> Lazy[Return[T] | VMError | UserError]:
	"""
	Runs a function of *this* contract in an isolated sandbox environment.

	The function is executed in a separate VM instance with controlled permissions.
	This provides isolation and security for potentially unsafe operations.
	Determinism of spawned VM matches the determinism of the current VM.

	Each ``allow_*`` flag grants the corresponding permission to the sandbox, but
	only if the current VM holds it as well.

	To run a *different* runner -- one that may not even be Python -- use
	:py:func:`spawn_runner`, which this is a thin wrapper over.

	:param fn: Function to execute in the sandbox (must be serializable with cloudpickle)
	:param allow_write_storage: Whether to allow storage writes in the sandbox
	:param allow_send_messages: Whether to allow sending messages in the sandbox
	:param custom_runners: ``custom:<hash>`` ids visible to the sandbox; ``None`` grants this VM's entire set, a list grants exactly that subset of it
	:param changes_on_error: see :py:obj:`SandboxChangesOnError`

	Example:
		>>> result = spawn_sandbox(lambda: risky_computation())
		>>> safe_value = unpack_result(result)
	"""
	import cloudpickle

	return typing.cast(
		Lazy[Return[T] | VMError | UserError],
		spawn_runner.lazy(
			RunnerID('contract'),
			cloudpickle.dumps(fn),
			allow_write_storage=allow_write_storage,
			allow_send_messages=allow_send_messages,
			custom_runners=custom_runners,
			changes_on_error=changes_on_error,
		),
	)


@_lazy_api
def run_nondet[T: calldata.Decoded](
	leader_fn: typing.Callable[[], T],
	validator_fn: typing.Callable[[Result], bool],
	/,
	*,
	custom_runners: list[RunnerID] | None = None,
	catch_vm_error: bool = False,
) -> Lazy[T]:
	"""
	Executes a non-deterministic block with leader-validator consensus.

	This is the most generic API for non-deterministic execution. The leader function
	runs as is, validators one checks the result.

	:param leader_fn: Function executed by the leader node (must be serializable)
	:param validator_fn: Function that validates the leader's result and returns bool
	:param custom_runners: ``custom:<hash>`` ids visible to the block; ``None`` grants this VM's entire set, a list grants exactly that subset of it
	:param catch_vm_error: return the block's VM error instead of re-raising it; a fatal one is never caught
	:return: The result from the leader (iff validation passes, otherwise VM will be terminated)

	.. warning::
		This function does not use extra sandbox for catching validator errors.
		Validator error will result in a ``Disagree`` error in executor (same as if
		this function returned ``False``). Use :py:func:`run_nondet_default` instead if you
		want to catch and inspect ``validator_fn`` errors, or use sandbox inside of it.

	.. note::
		All sub-vm returns go through :py:mod:`genlayer.calldata` encoding.

	Example:
		>>> def leader():
		...   return os.urandom(1)[0] % 3
		>>> def validator(result):
		...   return unpack_result(result) == 1  # agree in 33% of cases
		>>> value = gl.vm.run_nondet(leader, validator)
	"""
	import cloudpickle

	def validator_fn_mapped(stage_data):
		leaders_result = _decode_sub_vm_result_retn(stage_data['leaders_result'])
		return validator_fn(leaders_result)

	res = gl_call.gl_call_generic(
		{
			'RunNondet': {
				'data_leader': cloudpickle.dumps(lambda _: leader_fn()),
				'data_validator': cloudpickle.dumps(validator_fn_mapped),
				'custom_runners': custom_runners,
				'catch_vm_error': catch_vm_error,
			}
		},
		_decode_sub_vm_result,
	)

	return typing.cast(Lazy[T], res)


@_lazy_api
def run_nondet_default[T: calldata.Decoded](
	leader_fn: typing.Callable[[], T],
	validator_fn: typing.Callable[[Result[T]], bool],
	/,
	*,
	compare_user_errors: typing.Callable[[UserError, UserError], bool] = lambda a, b: (
		a.data == b.data
	),
	compare_vm_errors: typing.Callable[[VMError, VMError], bool] = lambda a, b: (
		a.public_code == b.public_code
	),
	custom_runners: list[RunnerID] | None = None,
	catch_vm_error: bool = False,
) -> Lazy[T]:
	"""
	Executes a non-deterministic block with comprehensive error handling.

	This is the recommended API for custom non-deterministic execution. It provides safer
	error handling compared to :py:func:`run_nondet` by running the validator
	in a sandbox and handling validator errors with provided functions with sensible defaults.

	:param leader_fn: Function executed by the leader node
	:param validator_fn: Function that validates the leader's result, is ran in a sandbox
	:param compare_user_errors: Function to compare UserError instances for equality
	:param compare_vm_errors: Function to compare VMError instances for equality; the default compares only the public code (the part before the first `` # `` detail suffix), ignoring implementation-specific diagnostics
	:param custom_runners: ``custom:<hash>`` ids visible to the block; ``None`` grants this VM's entire set, a list grants exactly that subset of it
	:param catch_vm_error: return the block's VM error instead of re-raising it; a fatal one is never caught
	:return: The result from the leader if validation passes

	Error handling:
	- If leader and validator both succeed: returns leader result
	- If leader fails and validator agrees: propagates leader error
	- If results don't match: consensus fails

	Example:
		>>> def leader() -> list[int]:
		...   return fetch_external_data()
		>>> def validator(result):
		...   if not isinstance(result, Return):
		...     return False
		...   my_data = leader()
		...   return (
		...     numpy.linalg.norm(np.array(result.calldata) - np.array(my_data)) < 0.1
		...   )
		>>> value = run_nondet_default(leader, validator)
	"""
	import cloudpickle

	def real_leader_fn(stage_data):
		assert stage_data is None
		return leader_fn()

	def real_validator_fn(stage_data) -> bool:
		leaders_result = _decode_sub_vm_result_retn(stage_data['leaders_result'])

		import genlayer.vm as vm

		answer = vm.spawn_sandbox(
			lambda: validator_fn(leaders_result),
			allow_write_storage=True,
			allow_send_messages=True,
		)

		if isinstance(answer, Return) and isinstance(leaders_result, Return):
			if not isinstance(answer.calldata, bool):
				raise TypeError(f'validator function returned non-bool `{answer.calldata}`')
			return answer.calldata
		if isinstance(answer, UserError) and isinstance(leaders_result, UserError):
			return compare_user_errors(leaders_result, answer)
		if isinstance(answer, VMError) and isinstance(leaders_result, VMError):
			return compare_vm_errors(leaders_result, answer)
		raise TypeError(
			f'validator function returned `{answer!r:20}` while leader returned `{leaders_result!r:20}`'
		)

	res = gl_call.gl_call_generic(
		{
			'RunNondet': {
				'data_leader': cloudpickle.dumps(real_leader_fn),
				'data_validator': cloudpickle.dumps(real_validator_fn),
				'custom_runners': custom_runners,
				'catch_vm_error': catch_vm_error,
			}
		},
		_decode_sub_vm_result,
	)

	return typing.cast(Lazy[T], res)


def trace(*objs: typing.Any, sep: str = ' '):
	wasi.gl_call(
		calldata.encode(
			{
				'Trace': {
					'Message': sep.join(str(obj) for obj in objs),
				},
			}
		)
	)


def trace_time_micro() -> int:
	return gl_call.gl_call_generic(
		{
			'Trace': {
				'RuntimeMicroSec': None,
			},
		},
		lambda x: typing.cast(int, calldata.decode(x)),
	).get()


def yield_() -> None:
	"""
	Cooperative yield. Currently a no-op, reserved for future use in waiting loops.
	"""
	wasi.gl_call(calldata.encode({'Yield': None}))


def get_timestamp() -> datetime.datetime:
	"""
	Returns the current timestamp as a timezone-aware ``datetime``.

	In deterministic mode it is the transaction timestamp; in
	non-deterministic mode it is the real wall-clock time.
	"""
	return gl_call.gl_call_generic(
		{
			'GetTimestamp': None,
		},
		lambda x: datetime.datetime.fromtimestamp(
			typing.cast(int, calldata.decode(x)), datetime.timezone.utc
		),
	).get()


def register_runner(code: collections.abc.Buffer) -> RunnerID:
	"""
	Registers a runner archive at runtime and returns its ``custom:<hash>`` id.

	The returned id can be referenced from ``Depends``/``With`` actions of other
	runners. Requires deterministic mode.

	:param code: runner archive bytes (zip, raw wasm or commented text)
	:return: the ``custom:<hash>`` runner id
	"""
	return gl_call.gl_call_generic(
		{
			'RegisterRunner': {
				'code': code,
			},
		},
		lambda x: typing.cast(RunnerID, calldata.decode(x)),
	).get()


def map_file(runner: RunnerID, path_in_runner: str, path_in_vfs: str) -> None:
	"""
	Maps a file from a runner into the VM filesystem at runtime.

	Behaves the same as the ``MapFile`` runner action: if ``path_in_runner`` ends
	with ``/`` the whole directory subtree is mapped, otherwise a single file.

	Requires the ``read_storage`` permission (a ``chain:`` runner reads another
	contract's storage). Mapping into ``/vm/`` is forbidden.

	:param runner: runner id (e.g. ``name:hash``, ``contract``, ``custom:<hash>``)
	:param path_in_runner: path within the runner archive
	:param path_in_vfs: absolute destination path in the VM filesystem
	"""
	wasi.gl_call(
		calldata.encode(
			{
				'MapFile': {
					'runner': runner,
					'path_in_runner': path_in_runner,
					'path_in_vfs': path_in_vfs,
				}
			}
		)
	)
