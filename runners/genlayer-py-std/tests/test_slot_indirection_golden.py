from genlayer.storage.core import ROOT_SLOT_ID, InmemManager


def test_slot_indirection_golden():
	"""
	Golden vector for the slot indirection derivation
	``sha3_256(slot_id || offset.to_le_bytes(4))``. The same constant is pinned in the
	Rust test (``executor/src/host/message.rs``) so the two derivations can never
	silently diverge.
	"""
	man = InmemManager()
	slot = man.get_store_slot(ROOT_SLOT_ID)
	derived = slot.indirect(2)
	assert (
		derived.id.hex()
		== 'ba005630745acf3014aaf162e9933040302ca0bef3f56fe2d73c0a08f82c610b'
	)
