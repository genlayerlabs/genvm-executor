__all__ = ('STORAGE_MAN', 'ROOT_SLOT_ID')

import collections.abc

import _genlayer_wasi as wasi

from genlayer.storage.core import ROOT_SLOT_ID, Manager, Slot, slot_id_to_bytes
from genlayer.types import u256


class _ActualStorageMan(Manager):
	__slots__ = ('_slots',)

	_slots: dict[bytes, Slot]

	def __init__(self):
		self._slots = {}

	def get_store_slot(self, addr: bytes | u256) -> Slot:
		addr = slot_id_to_bytes(addr)
		ret = self._slots.get(addr, None)
		if ret is None:
			ret = Slot(addr, self)
			self._slots[addr] = ret
		return ret

	def do_read(self, id: bytes, off: int, len: int) -> bytes:
		res = bytearray(len)
		wasi.storage_read(id, off, res)
		return bytes(res)

	def do_write(self, id: bytes, off: int, what: collections.abc.Buffer) -> None:
		wasi.storage_write(id, off, what)


STORAGE_MAN = _ActualStorageMan()
"""
Storage slots manager that provides an access to the "Host" (node) state
"""

from genlayer.storage import Root  # noqa: E402

Root.MANAGER = STORAGE_MAN
