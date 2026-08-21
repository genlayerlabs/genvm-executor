__all__ = ('Root',)

import typing

from genlayer.storage.core import (
	ROOT_SLOT_ID,
	VLA,
	Indirection,
	Manager,
	Slot,
)
from genlayer.types import Address, u8, u256
from genlayer.vm import public_abi

from ._internal.generate import InmemManager, _known_descs, generate_storage


@generate_storage
class Root:
	"""
	This ABI is known and used by:

	#. genvm
	#. node
	"""

	MANAGER: typing.ClassVar[Manager] = InmemManager()
	"""
	Manager instance for the storage.

	It is set to an actual storage manager in the runtime, but can be overridden for testing purposes.
	"""

	major: u8
	"""
	Major version of the GenVM contract expects
	"""

	contract_instance: Indirection[None]

	code: Indirection[VLA[u8]]
	"""
	contract code
	"""
	locked_slots: Indirection[VLA[u256]]
	"""
	Slot ids that can not be modified after deployment. Use :py:func:`Slot.as_int` for conversion of Slot to :py:class:`int`
	By default it will be populated by ``code``, ``frozen_slots``
	"""
	upgraders: Indirection[VLA[Address]]

	code_slot: u256
	"""
	Slot id the contract code is stored at, as a raw 32-byte value. If zero (the
	default), the code is read from the default ``code`` slot; otherwise it is read
	from the pointed-to slot (see ``chain:`` runner ids).
	"""

	permissions: u256
	"""
	Permission bitfield read by the executor at execution start. Bit ``n`` corresponds
	to the :py:class:`~genlayer.vm.public_abi.Permissions` member whose value is ``n``.
	Use :py:meth:`get_permission` / :py:meth:`set_permission` to access it.
	"""

	@staticmethod
	def get() -> 'Root':
		"""
		Return the root storage instance.

		:returns: singleton root object
		"""
		slot = Root.MANAGER.get_store_slot(ROOT_SLOT_ID)
		return _known_descs[Root].get(slot, 0)

	def slot(self) -> Slot:
		"""
		Return the storage slot backing this root.

		:returns: underlying storage slot
		"""
		return self._storage_slot  # type: ignore

	def get_resolved_code(self) -> VLA[u8]:
		"""
		Return the storage slot where the contract code is stored.

		:returns: code storage slot
		"""
		if self.code_slot == 0:
			return self.code.get()
		else:
			return Root.MANAGER.get_store_slot(self.code_slot).cast(VLA[u8], 0)

	def get_vacant_slot(self) -> Slot:
		"""
		This slot can be used to store data without worrying about overwriting contract data.
		Useful for bootstrapping contract storage
		"""
		return self.slot().indirect(public_abi.root_offsets.CODE_SLOT)

	def get_contract_instance[T](self, typ: typing.Type[T], /) -> T:
		"""
		Return the contract instance deserialized as the given type.

		:param typ: storage-allowed type to deserialize into
		:returns: contract instance
		"""
		slot: Slot = self.slot().indirect(public_abi.root_offsets.CONTRACT)
		return _known_descs[typ].get(slot, 0)

	def lock_default(self):
		"""
		Lock the default set of slots (root, code, locked_slots, upgraders) to prevent modification after deployment.

		Slots are appended in that order. If an append fails and the exception is
		caught, earlier appends and the failed append's length change remain.
		"""
		frozen = self.locked_slots.get()

		frozen.append(self.slot().as_int())
		frozen.append(self.code.slot().as_int())
		frozen.append(self.locked_slots.slot().as_int())
		frozen.append(self.upgraders.slot().as_int())

	def get_permission(self, perm: public_abi.Permissions, /) -> bool:
		"""
		Check whether a permission bit is set in the root ``permissions`` bitfield.

		:param perm: permission to query
		:returns: ``True`` iff the corresponding bit is set
		"""
		return (self.permissions >> perm.value) & 1 != 0

	def set_permission(self, perm: public_abi.Permissions, value: bool, /) -> None:
		"""
		Set or clear a permission bit in the root ``permissions`` bitfield.

		:param perm: permission to modify
		:param value: whether to grant the permission

		This method performs one fixed-size storage-field write.
		"""
		bit = 1 << perm.value
		if value:
			self.permissions |= bit
		else:
			self.permissions &= ~bit
