#!/usr/bin/env python3

import sys
from pathlib import Path

sys.path.append(str(Path(__file__).parent.parent))
import typing

import numpy as np
from fuzz_common import FuzzerBuilder, StopFuzzingException, do_fuzzing
from genlayer.storage import inmem_allocate
from genlayer.types import u32
from genlayer_embeddings import EuclideanDistance, VecDB
from genlayer_embeddings.vecdb import MAX_LEVEL, MIN_LEVEL, NO_PARENT


def check_cover_tree_invariants(db: VecDB) -> None:
	"""
	Check the compressed representation of all three cover-tree invariants.
	"""
	if len(db) == 0:
		assert db._root_idx == NO_PARENT
		assert len(db._elem_to_node) == 0
		return

	assert db._root_idx != NO_PARENT
	if db._root_idx in db._free_nodes:
		print(f'STRUCTURAL VIOLATION: root node {db._root_idx} is in free nodes')
		assert False

	nodes: list[tuple[int, int, int]] = []
	reachable_nodes: set[int] = set()
	reachable_elements: set[int] = set()
	stack = [db._root_idx]
	while stack:
		node_idx = stack.pop()
		if node_idx in reachable_nodes:
			print(f'STRUCTURAL VIOLATION: node {node_idx} is reachable more than once')
			assert False
		reachable_nodes.add(node_idx)
		node = db._nodes[node_idx]
		level = int(node.level)
		elem_id = int(node.element_id)
		if elem_id in db._free_idx:
			print(
				f'STRUCTURAL VIOLATION: active node {node_idx} references freed element {elem_id}'
			)
			assert False
		assert db._elem_to_node[elem_id] == node_idx
		reachable_elements.add(elem_id)
		for i in range(len(node.duplicates)):
			duplicate_id = int(node.duplicates[i])
			assert duplicate_id not in reachable_elements
			assert duplicate_id not in db._free_idx
			assert db._elem_to_node[duplicate_id] == node_idx
			assert db._duplicate_pos[duplicate_id] == i
			assert db._distance(elem_id, duplicate_id) == 0
			reachable_elements.add(duplicate_id)
		nodes.append((node_idx, elem_id, level))

		for i in range(len(node.children)):
			child_idx = node.children[i]
			if child_idx in db._free_nodes:
				print(
					f'STRUCTURAL VIOLATION: node {node_idx} (element {elem_id}, level {level}) has freed child {child_idx}'
				)
				assert False
			stack.append(child_idx)

	assert len(reachable_elements) == len(db)
	assert len(db._elem_to_node) == len(db)
	assert len(db._duplicate_pos) == len(db) - len(reachable_nodes)
	assert len(reachable_nodes) == len(db._nodes) - len(db._free_nodes)
	root = db._nodes[db._root_idx]
	assert root.parent == NO_PARENT
	assert int(root.level) == MAX_LEVEL
	level_counts: dict[int, int] = {}
	for _, _, level in nodes:
		level_counts[level] = level_counts.get(level, 0) + 1
	assert dict(db._level_counts.items()) == level_counts
	assert int(db._min_level) == min(level_counts)
	assert int(db._max_level) == max(level_counts)

	# Nesting: a compressed node represents its point at every lower level
	for node_idx, _, level in nodes:
		assert MIN_LEVEL <= level <= MAX_LEVEL
		node = db._nodes[node_idx]
		for i in range(len(node.children)):
			child = db._nodes[node.children[i]]
			if child.parent != node_idx or int(child.level) >= level:
				print('NESTING INVARIANT VIOLATION: invalid compressed parent edge')
				assert False

	# Covering: a child first appears at level i and is covered in C_(i+1)
	for node_idx, elem_id, _ in nodes:
		node = db._nodes[node_idx]
		for i in range(len(node.children)):
			child = db._nodes[node.children[i]]
			distance = float(db._distance(elem_id, child.element_id))
			max_distance = db._radius(int(child.level) + 1)
			if distance > max_distance:
				print('COVERING INVARIANT VIOLATION:')
				print(f'  Distance {distance} > {max_distance}')
				assert False

	# Separation: C_i contains every explicit point whose maximum level is >= i
	for i, (_, elem_id, level) in enumerate(nodes):
		for _, other_id, other_level in nodes[i + 1 :]:
			check_level = min(level, other_level)
			distance = float(db._distance(elem_id, other_id))
			min_distance = db._radius(check_level)
			if distance <= min_distance:
				print('GLOBAL SEPARATING INVARIANT VIOLATION:')
				print(f'  Distance {distance} <= {min_distance} at level {check_level}')
				assert False


class Etalon:
	data: np.ndarray[tuple[int, typing.Literal[5]], np.dtype[np.float32]]
	vals: list[u32]

	def __init__(self):
		self.data = np.empty((0, 5), dtype=np.float32)
		self.vals = []

	def add(self, key: np.ndarray[tuple[typing.Literal[5]], np.dtype[np.float32]], val):
		self.data = np.vstack([self.data, key])
		self.vals.append(val)


def vec_db_2(buf):
	builder = FuzzerBuilder(buf)

	def finite_float(num):
		i = 0
		while i < num:
			f = builder.fetch_float()
			if not np.isfinite(f):
				continue
			if abs(f) > 1e5:
				f = np.fmod(f, 1e5 + 3)
			yield f
			i += 1

	def gen_vec() -> np.ndarray:
		return np.array(list(finite_float(5))).astype(np.float32)

	try:
		etalon = Etalon()
		db = inmem_allocate(VecDB[np.float32, typing.Literal[5], u32, EuclideanDistance])

		id_to_value: dict[VecDB.Id, u32] = {}

		cnt = builder.fetch(1)[0] % 80 + 10

		steps = []

		for i in range(cnt):
			c = builder.fetch(1)[0] % 3
			if len(db) == 0 and c == 0:
				c = 1

			match c:
				case 0:
					db_id, val = id_to_value.popitem()
					elem = db.get_by_id(db_id)
					steps.append(f'Remove element {elem.value}')
					elem.remove()
					rem_idx = etalon.vals.index(val)
					etalon.data = np.delete(etalon.data, rem_idx, axis=0)
					etalon.vals.pop(rem_idx)

					try:
						check_cover_tree_invariants(db)
					except AssertionError:
						print('=== steps ===')
						for step in steps:
							print(step)
						raise
				case 1:
					key = gen_vec()
					db_id = db.insert(key, i)
					etalon.add(key, i)

					id_to_value[db_id] = i

					steps.append(f'Add element {key}')

					try:
						check_cover_tree_invariants(db)
					except AssertionError:
						print('=== steps ===')
						for step in steps:
							print(step)
						raise
				case 2:
					query_around = gen_vec()

					k = builder.fetch(1)[0] % 3 + 3

					got = list((x.distance, x.value) for x in db.knn(query_around, k))
					got.sort(key=lambda x: x[0])

					d = EuclideanDistance()
					distances = d.batch(etalon.data, query_around)
					closest_indices = np.argsort(distances)[:k]

					exp = list((distances[i], etalon.vals[i]) for i in closest_indices)
					exp.sort(key=lambda x: x[0])

					norm = np.linalg.norm(
						np.array(list(x[0] for x in exp)) - np.array(list(x[0] for x in got))
					)

					if norm > 1e-5:
						print(f'k: {k}')
						print(f'query: {query_around}')
						print(f'expected: {exp}')
						print(f'got: {got}')

						for x in db:
							print(f'  {x.key} -> {x.value}')
							for y in got:
								if x.value == y[1]:
									print(f'    ^^^^ found in got {y}')
							if any(x.value == y[1] for y in exp):
								for y in exp:
									if x.value == y[1]:
										print(f'    ^^^^ found in exp {y}')

						assert False

	except StopFuzzingException:
		return


if __name__ == '__main__':
	do_fuzzing(vec_db_2)
