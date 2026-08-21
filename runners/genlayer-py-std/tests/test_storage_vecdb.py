import typing

import numpy as np
import pytest
from genlayer.storage._internal.generate import generate_storage
from genlayer_embeddings import EuclideanDistance, VecDB
from genlayer_embeddings.vecdb import NO_PARENT


@generate_storage
class DB:
	x: VecDB[np.int32, typing.Literal[5], str, EuclideanDistance]


def _vec(coord: int) -> np.ndarray:
	return np.array([coord, 0, 0, 0, 0], dtype=np.int32)


def _assert_cover_tree(db: VecDB) -> set[int]:
	if len(db) == 0:
		assert db._root_idx == NO_PARENT
		assert len(db._elem_to_node) == 0
		return set()

	reachable_nodes = set()
	reachable_elements = set()
	levels = []
	nodes = []
	stack = [db._root_idx]
	assert db._nodes[db._root_idx].parent == NO_PARENT
	while stack:
		parent_idx = stack.pop()
		assert parent_idx not in reachable_nodes
		assert parent_idx not in db._free_nodes
		reachable_nodes.add(parent_idx)

		parent = db._nodes[parent_idx]
		reachable_elements.add(int(parent.element_id))
		reachable_elements.update(
			int(parent.duplicates[i]) for i in range(len(parent.duplicates))
		)
		levels.append(int(parent.level))
		nodes.append(parent)
		assert db._elem_to_node[parent.element_id] == parent_idx
		for i in range(len(parent.duplicates)):
			assert db._elem_to_node[parent.duplicates[i]] == parent_idx
			assert db._duplicate_pos[parent.duplicates[i]] == i

		children = [db._nodes[parent.children[i]] for i in range(len(parent.children))]
		stack.extend(parent.children[i] for i in range(len(parent.children)))
		for child in children:
			assert child.parent == parent_idx
			assert child.level < parent.level
			assert db._distance(parent.element_id, child.element_id) <= (
				db._radius(int(child.level) + 1)
			)

	for i, node in enumerate(nodes):
		for other in nodes[i + 1 :]:
			level = min(int(node.level), int(other.level))
			assert db._distance(node.element_id, other.element_id) > db._radius(level)

	assert len(reachable_elements) == len(db)
	assert len(db._elem_to_node) == len(db)
	assert len(db._duplicate_pos) == len(db) - len(reachable_nodes)
	assert len(reachable_nodes) == len(db._nodes) - len(db._free_nodes)
	assert dict(db._level_counts.items()) == {
		level: levels.count(level) for level in set(levels)
	}
	assert db._min_level == min(levels)
	assert db._max_level == max(levels)
	return reachable_elements


def test_store_inv_shape():
	db = DB()

	with pytest.raises(Exception):
		ins_val = np.array([1], dtype=np.int32)
		db.x.insert(ins_val, '1')


def test_store_inv_type():
	db = DB()

	with pytest.raises(Exception):
		ins_val = np.array([1, 2, 3, 4, 5], dtype=np.float32)
		db.x.insert(ins_val, '1')  # type: ignore


def test_store_simple_ok():
	db = DB()

	ins_val = np.array([1, 2, 3, 4, 5], dtype=np.int32)
	db.x.insert(ins_val, '1')


def test_store_ids():
	db = DB()

	data = {
		'k1': '1',
		'k2': '2',
	}

	id_to_data_key: dict[str, VecDB.Id] = {}

	for k, v in data.items():
		id_to_data_key[k] = db.x.insert(np.array([0] * 5, dtype=np.int32), v)
	for k, v in data.items():
		db.x.get_by_id(id_to_data_key[k]).remove()
		id_to_data_key[k] = db.x.insert(np.array([0] * 5, dtype=np.int32), v)

	for k, v in id_to_data_key.items():
		assert db.x.get_by_id(v).id == v
		assert db.x.get_by_id(v).value == data[k]

	for it in db.x:
		assert it.value in data.values()


def test_store_knn():
	db = DB()

	ins_val = np.array([0, 0, 0, 0, 0], dtype=np.int32)
	db.x.insert(ins_val, '0')
	ins_val = np.array([1, 0, 0, 0, 0], dtype=np.int32)
	db.x.insert(ins_val, '1')
	ins_val = np.array([2, 0, 0, 0, 0], dtype=np.int32)
	db.x.insert(ins_val, '2')

	seen = set()
	for elem in db.x.knn(np.array([0, 0, 0, 0, 0], dtype=np.int32), 1):
		seen.add(elem.value)
	assert seen == set(['0'])

	seen = set()
	for elem in db.x.knn(np.array([0, 0, 0, 0, 0], dtype=np.int32), 2):
		seen.add(elem.value)
	assert seen == set(['0', '1'])

	seen = set()
	for elem in db.x.knn(np.array([0, 0, 0, 0, 0], dtype=np.int32), 3):
		seen.add(elem.value)
	assert seen == set(['0', '1', '2'])

	seen = set()
	for elem in db.x.knn(np.array([0, 0, 0, 0, 0], dtype=np.int32), 8):
		seen.add(elem.value)
	assert seen == set(['0', '1', '2'])


def test_remove_preserves_cover_tree_invariants():
	db = DB()
	ids = {
		coord: db.x.insert(np.array([coord, 0, 0, 0, 0], dtype=np.int32), str(coord))
		for coord in [0, 100, 90, 80, 86]
	}

	removed_node_idx = db.x._elem_to_node[ids[90]]
	descendant_nodes = set()
	stack = [
		db.x._nodes[removed_node_idx].children[i]
		for i in range(len(db.x._nodes[removed_node_idx].children))
	]
	while stack:
		node_idx = stack.pop()
		descendant_nodes.add(node_idx)
		node = db.x._nodes[node_idx]
		stack.extend(node.children[i] for i in range(len(node.children)))

	db.x.get_by_id(ids[90]).remove()

	assert {elem.value for elem in db.x} == {'0', '100', '80', '86'}
	assert _assert_cover_tree(db.x) == {ids[0], ids[100], ids[80], ids[86]}
	assert removed_node_idx in db.x._free_nodes
	assert descendant_nodes.isdisjoint(db.x._free_nodes)


def test_remove_root_leaf_and_last_element():
	db = DB()
	coords = [0, 100, 90, 80, 86]
	for coord in coords:
		db.x.insert(_vec(coord), str(coord))

	root_id = db.x._nodes[db.x._root_idx].element_id
	root_coord = int(db.x._values[root_id])
	db.x.get_by_id(root_id).remove()
	coords.remove(root_coord)
	_assert_cover_tree(db.x)

	leaf_id = next(
		element_id
		for element_id, node_idx in db.x._elem_to_node.items()
		if len(db.x._nodes[node_idx].children) == 0
	)
	leaf_coord = int(db.x._values[leaf_id])
	db.x.get_by_id(leaf_id).remove()
	coords.remove(leaf_coord)
	_assert_cover_tree(db.x)

	reused_id = db.x.insert(_vec(42), '42')
	assert reused_id in {root_id, leaf_id}
	coords.append(42)
	_assert_cover_tree(db.x)

	for query in [-10, 40, 110]:
		got = next(db.x.knn(_vec(query), 1))
		expected = min(coords, key=lambda coord: (coord - query) ** 2)
		assert got.value == str(expected)

	for element_id in [elem.id for elem in db.x]:
		db.x.get_by_id(element_id).remove()
		_assert_cover_tree(db.x)


def test_randomized_mutations_preserve_invariants_and_knn():
	rng = np.random.default_rng(7321)
	db = DB()
	active: dict[int, np.ndarray] = {}
	for step in range(300):
		if len(active) == 0 or rng.random() < 0.65:
			key = rng.integers(-100, 101, size=5, dtype=np.int32)
			if step % 17 == 0 and len(active) > 0:
				key = active[next(iter(active))].copy()
			element_id = int(db.x.insert(key, str(step)))
			active[element_id] = key
		else:
			element_id = int(rng.choice(list(active)))
			db.x.get_by_id(element_id).remove()  # type: ignore[arg-type]
			del active[element_id]

		_assert_cover_tree(db.x)
		if len(active) > 0:
			query = rng.integers(-100, 101, size=5, dtype=np.int32)
			k = min(5, len(active))
			got = sorted(elem.distance for elem in db.x.knn(query, k))
			expected = sorted(float(db.x._dist_func(key, query)) for key in active.values())[
				:k
			]
			assert got == expected


def test_remove_arbitrary_duplicate():
	db = DB()
	ids = [db.x.insert(_vec(7), str(i)) for i in range(4)]
	db.x.get_by_id(ids[1]).remove()

	assert {elem.value for elem in db.x} == {'0', '2', '3'}
	assert [elem.distance for elem in db.x.knn(_vec(7), 3)] == [0.0, 0.0, 0.0]
	_assert_cover_tree(db.x)
