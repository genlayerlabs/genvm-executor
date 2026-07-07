from genlayer.storage._internal.generate import _known_descs
from genlayer.storage.root import Root
from genlayer.vm import public_abi


def test_root_offsets_pinned():
	"""Pin the numeric root-slot offsets (mirrors the Rust ``root_offsets`` test)."""
	ro = public_abi.root_offsets
	assert (
		ro.MAJOR,
		ro.CONTRACT,
		ro.CODE,
		ro.LOCKED_SLOTS,
		ro.UPGRADERS,
		ro.CODE_SLOT,
		ro.PERMISSIONS,
	) == (0, 1, 2, 3, 4, 5, 37)


def test_computed_root_layout_matches_public_abi():
	"""
	The computed ``Root`` storage layout must agree with ``public_abi.root_offsets``, so
	any future field reorder or size change trips this test.
	"""
	ro = public_abi.root_offsets
	expected = {
		'major': ro.MAJOR,
		'contract_instance': ro.CONTRACT,
		'code': ro.CODE,
		'locked_slots': ro.LOCKED_SLOTS,
		'upgraders': ro.UPGRADERS,
		'code_slot': ro.CODE_SLOT,
		'permissions': ro.PERMISSIONS,
	}

	props = _known_descs[Root].props  # name -> (TypeDesc, byte offset)
	for name, off in expected.items():
		assert props[name][1] == off, f'offset mismatch for `{name}`'
