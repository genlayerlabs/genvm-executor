from __future__ import annotations

__all__ = (
	'VecDB',
	'VecDBElement',
	'Distance',
	'EuclideanDistance',
	'ManhattanDistance',
	'ChebyshevDistance',
)

import math
import typing

import numpy as np
from genlayer.storage import DynArray, TreeMap, allow
from genlayer.types import i32, u32


class Distance(typing.Protocol):
	"""
	Protocol for distance functions used by :py:class:`VecDB`.

	Implementations must be a true metric (non-negative, symmetric, zero iff
	equal, and satisfying the triangle inequality); otherwise the cover-tree
	pruning in :py:meth:`VecDB.knn` may skip the true nearest neighbor.
	"""

	def __call__(self, l, r) -> typing.Any:
		"""
		Compute the distance between two vectors.

		:param l: left-hand vector
		:param r: right-hand vector
		:returns: distance between ``l`` and ``r``
		"""
		...


@allow
class EuclideanDistance(Distance):
	def __call__(self, l, r):
		return np.sqrt(np.sum((l - r) ** 2))

	def batch(self, l, r):
		return np.sqrt(((l - r) ** 2).sum(axis=1))


@allow
class ManhattanDistance(Distance):
	"""L1 (taxicab) distance. A true metric, safe for cover-tree pruning."""

	def __call__(self, l, r):
		return np.sum(np.abs(l - r))

	def batch(self, l, r):
		return np.abs(l - r).sum(axis=1)


@allow
class ChebyshevDistance(Distance):
	"""L-infinity (max-coordinate) distance. A true metric, safe for pruning."""

	def __call__(self, l, r):
		return np.max(np.abs(l - r))

	def batch(self, l, r):
		return np.abs(l - r).max(axis=1)


Id = typing.NewType('Id', int)
_Id = Id

NO_PARENT: u32 = 0xFFFFFFFF  # Constant for no parent node
MIN_LEVEL: i32 = -1075  # One level below the smallest positive float64
MAX_LEVEL: i32 = 65535  # Cap for root level


@allow
class CoverTreeNode:
	"""A node in the cover tree structure"""

	element_id: u32
	level: i32
	children: DynArray[u32]  # Indices of child nodes
	parent: u32  # Index of parent node, NO_PARENT if root
	duplicates: DynArray[u32]  # Other elements at the same metric point

	def __init__(self, element_id: u32, level: i32):
		self.element_id = element_id
		self.level = level


class VecDBElement[T: np.number, S: int, V, Dist]:
	distance: Dist
	"""
	Distance from search point to this element, if any
	"""

	__slots__ = ('_idx', '_db', 'distance')

	def __init__(self, db: VecDB[T, S, V, typing.Any], idx: u32, distance: Dist):
		self._idx = idx
		self._db = db
		self.distance = distance

	def __repr__(self) -> str:
		return f'VecDB.Element(id={self.id!r}, key={self.key!r}, value={self.value!r}, distance={self.distance})'

	@property
	def key(self) -> np.ndarray[tuple[S], np.dtype[T]]:
		"""
		Key (vector) of this element
		"""
		return self._db._keys[self._idx]

	@property
	def id(self) -> Id:
		"""
		Id (unique key) of this element
		"""
		return Id(self._idx)

	@property
	def value(self) -> V:
		"""
		Value of this element
		"""
		return self._db._values[self._idx]

	@value.setter
	def value(self, v: V):
		self._db._values[self._idx] = v

	def remove(self) -> None:
		"""
		Removes current element from the db
		"""
		self._db._remove_from_tree(self._idx)
		self._db._free_idx[self._idx] = None


@allow
class VecDB[T: np.number, S: int, V, D: Distance]:
	"""
	Data structure that supports storing and querying vector data using Cover Trees

	Cover trees provide logarithmic time nearest neighbor search with theoretical guarantees.

	There are two entities that can act as a key:

	#. vector (can have duplicates)
	#. id (int alias, can't have duplicates)

	.. warning::
		import :py:mod:`numpy` before ``from genlayer import *`` if you wish to use :py:class:`VecDB`!
	"""

	type Id = _Id
	"""
	:py:class:`int` alias to prevent confusion
	"""

	type Element = VecDBElement
	"""
	Shorthand to prevent global namespace pollution
	"""

	_keys: DynArray[np.ndarray[tuple[S], np.dtype[T]]]
	_values: DynArray[V]
	_free_idx: TreeMap[u32, None]
	_nodes: DynArray[CoverTreeNode]
	_free_nodes: TreeMap[u32, None]
	_elem_to_node: TreeMap[u32, u32]  # element_id -> highest-level node_idx
	_root_idx: u32
	_base: float
	_max_level: i32
	_min_level: i32
	_dist_func: D

	_initialized: bool = False
	_level_counts: TreeMap[i32, u32]
	_tree_version: u32
	_duplicate_pos: TreeMap[u32, u32]

	def __init__(self):
		self._do_init()

	def _do_init(self):
		if not self._initialized:
			self._initialized = True
			self._root_idx = NO_PARENT
			self._base = 2.0
			self._max_level = 0
			self._min_level = 0
			self._tree_version = 1
			return
		if self._tree_version == 1:
			return
		self._rebuild_legacy_tree()

	def _rebuild_legacy_tree(self) -> None:
		element_ids = [i for i in range(len(self._keys)) if i not in self._free_idx]
		self._nodes.clear()
		self._free_nodes.clear()
		self._elem_to_node.clear()
		self._level_counts.clear()
		self._duplicate_pos.clear()
		self._root_idx = NO_PARENT
		self._base = 2.0
		self._max_level = 0
		self._min_level = 0
		self._tree_version = 1
		for element_id in element_ids:
			self._insert_into_tree(element_id)

	def __len__(self) -> int:
		self._do_init()
		return len(self._keys) - len(self._free_idx)

	def get_by_id(self, id: Id) -> VecDBElement[T, S, V, None]:
		res = self.get_by_id_or_none(id)
		if res is None:
			raise KeyError(f'no element with id {id}')
		return res

	def get_by_id_or_none(self, id: Id) -> VecDBElement[T, S, V, None] | None:
		self._do_init()
		if id < 0 or id >= len(self._keys):
			return None
		if id in self._free_idx:
			return None
		return VecDBElement(self, id, None)

	def _distance(self, idx1: u32, idx2: u32) -> T:
		"""Compute distance between two elements by their indices"""
		return self._dist_func(self._keys[idx1], self._keys[idx2])

	def _distance_to_point(self, idx: u32, point: np.ndarray[tuple[S], np.dtype[T]]) -> T:
		"""Compute distance from element to query point"""
		return self._dist_func(self._keys[idx], point)

	def _allocate_node(self, element_id: u32, level: i32) -> u32:
		"""Allocate a new node and return its index"""
		if len(self._free_nodes) > 0:
			node_idx = self._free_nodes.popitem()[0]
			self._nodes[node_idx] = CoverTreeNode(element_id, level)
			return node_idx
		else:
			node = CoverTreeNode(element_id, level)
			self._nodes.append(node)
			return len(self._nodes) - 1

	def _free_node(self, node_idx: u32) -> None:
		"""Mark a node as free"""
		self._free_nodes[node_idx] = None

	def _radius(self, level: int) -> float:
		try:
			return self._base**level
		except OverflowError:
			return float('inf')

	def _add_level(self, level: i32) -> None:
		if level in self._level_counts:
			self._level_counts[level] += 1
		else:
			self._level_counts[level] = 1
		if len(self._level_counts) == 1 or level < self._min_level:
			self._min_level = level
		if level > self._max_level:
			self._max_level = level

	def _remove_level(self, level: i32) -> None:
		count = self._level_counts[level]
		if count > 1:
			self._level_counts[level] = count - 1
			return
		del self._level_counts[level]
		if len(self._level_counts) == 0:
			self._min_level = 0
			self._max_level = 0
		else:
			self._min_level = next(iter(self._level_counts))
			self._max_level = max(self._level_counts)

	def _set_node_level(self, node_idx: u32, level: i32) -> None:
		node = self._nodes[node_idx]
		if node.level == level:
			return
		self._remove_level(node.level)
		node.level = level
		self._add_level(level)

	def insert(self, key: np.ndarray[tuple[S], np.dtype[T]], val: V) -> Id:
		self._do_init()
		# Add to storage arrays
		if len(self._free_idx) > 0:
			idx = self._free_idx.popitem()[0]
			self._keys[idx] = key
			self._values[idx] = val
		else:
			self._keys.append(key)
			self._values.append(val)
			idx = len(self._keys) - 1

		# Insert into cover tree
		self._insert_into_tree(idx)

		return Id(idx)

	def _level_for_dist(self, dist: float) -> i32:
		"""
		Compute the cover tree level for a given distance.

		Returns largest L such that base^L < dist (i.e. dist > base^L).
		This ensures the separating invariant: points at level L are > base^L apart.
		"""
		if dist <= 0:
			return MIN_LEVEL
		return int(math.ceil(math.log(dist) / math.log(self._base))) - 1

	def _insert_into_tree(self, new_idx: u32) -> None:
		"""Insert an element using Algorithm 2 of the cover-tree paper"""
		if self._root_idx == NO_PARENT:
			self._root_idx = self._allocate_node(new_idx, MAX_LEVEL)
			self._nodes[self._root_idx].parent = NO_PARENT
			self._add_level(MAX_LEVEL)
			self._elem_to_node[new_idx] = self._root_idx
			return

		root_node = self._nodes[self._root_idx]
		root_dist = float(self._distance(new_idx, root_node.element_id))
		if root_dist == 0:
			root_node.duplicates.append(new_idx)
			self._duplicate_pos[new_idx] = len(root_node.duplicates) - 1
			self._elem_to_node[new_idx] = self._root_idx
			return

		top_level = self._level_for_dist(root_dist) + 1
		for i in range(len(root_node.children)):
			top_level = max(top_level, int(self._nodes[root_node.children[i]].level) + 1)
		top_level = min(top_level, MAX_LEVEL - 1)

		candidates: list[u32] = [self._root_idx]
		frames: list[tuple[int, list[u32]]] = []
		level = top_level
		while level > MIN_LEVEL:
			frames.append((level, candidates))
			expanded = self._children_at_level(candidates, level - 1)
			next_candidates: list[u32] = []
			min_dist = float('inf')
			duplicate_idx = NO_PARENT
			for node_idx in expanded:
				dist = float(self._distance(new_idx, self._nodes[node_idx].element_id))
				if dist == 0:
					duplicate_idx = node_idx
					break
				if dist < min_dist:
					min_dist = dist
				if dist <= self._radius(level):
					next_candidates.append(node_idx)
			if duplicate_idx != NO_PARENT:
				duplicates = self._nodes[duplicate_idx].duplicates
				duplicates.append(new_idx)
				self._duplicate_pos[new_idx] = len(duplicates) - 1
				self._elem_to_node[new_idx] = duplicate_idx
				return
			if min_dist > self._radius(level):
				break
			candidates = next_candidates
			level -= 1

		for parent_level, parent_candidates in reversed(frames):
			nearest_idx = NO_PARENT
			nearest_dist = float('inf')
			for node_idx in parent_candidates:
				dist = float(self._distance(new_idx, self._nodes[node_idx].element_id))
				if dist <= self._radius(parent_level) and dist < nearest_dist:
					nearest_idx = node_idx
					nearest_dist = dist
			if nearest_idx == NO_PARENT:
				continue
			new_level = max(parent_level - 1, MIN_LEVEL)
			new_node_idx = self._allocate_node(new_idx, new_level)
			self._nodes[new_node_idx].parent = nearest_idx
			self._nodes[nearest_idx].children.append(new_node_idx)
			self._add_level(new_level)
			self._elem_to_node[new_idx] = new_node_idx
			return

		raise RuntimeError('cover tree could not find an insertion parent')

	def _children_at_level(self, candidates: list[u32], level: int) -> list[u32]:
		result: list[u32] = []
		seen: set[u32] = set()
		for node_idx in candidates:
			if node_idx not in seen:
				seen.add(node_idx)
				result.append(node_idx)
			node = self._nodes[node_idx]
			for i in range(len(node.children)):
				child_idx = node.children[i]
				if self._nodes[child_idx].level == level and child_idx not in seen:
					seen.add(child_idx)
					result.append(child_idx)
		return result

	def _remove_from_tree(self, idx: u32) -> None:
		"""Remove an element using Algorithm 3 of the cover-tree paper"""
		if idx in self._elem_to_node:
			node_idx = self._elem_to_node[idx]
		else:
			# Fallback for legacy data without _elem_to_node populated
			node_idx = self._find_node_by_id(idx)
			if node_idx == NO_PARENT:
				return

		node = self._nodes[node_idx]
		if node.element_id != idx or len(node.duplicates) > 0:
			self._remove_duplicate(node_idx, idx)
			return

		orphans = [node.children[i] for i in range(len(node.children))]
		min_parent_level = int(node.level)
		if len(orphans) > 0:
			min_parent_level = min(int(self._nodes[x].level) + 1 for x in orphans)
		cover_sets, top_level = self._removal_cover_sets(idx, min_parent_level)

		if node.parent != NO_PARENT:
			self._remove_child(node.parent, node_idx)
		else:
			if len(orphans) == 0:
				self._root_idx = NO_PARENT
			else:
				new_root_idx = max(orphans, key=lambda x: int(self._nodes[x].level))
				orphans.remove(new_root_idx)
				self._nodes[new_root_idx].parent = NO_PARENT
				self._set_node_level(new_root_idx, MAX_LEVEL)
				self._root_idx = new_root_idx
				for _, candidates in cover_sets:
					if new_root_idx not in candidates:
						candidates.append(new_root_idx)

		for orphan_idx in orphans:
			self._nodes[orphan_idx].parent = NO_PARENT
		node.children[:] = []
		del self._elem_to_node[idx]
		self._remove_level(node.level)
		self._free_node(node_idx)

		for orphan_idx in sorted(
			orphans, key=lambda x: int(self._nodes[x].level), reverse=True
		):
			self._adopt_orphan(orphan_idx, node_idx, idx, cover_sets, top_level)

	def _remove_duplicate(self, node_idx: u32, idx: u32) -> None:
		node = self._nodes[node_idx]
		if node.element_id == idx:
			replacement = node.duplicates[-1]
			node.duplicates.pop()
			del self._duplicate_pos[replacement]
			node.element_id = replacement
		else:
			position = self._duplicate_pos[idx]
			last_position = len(node.duplicates) - 1
			last_element = node.duplicates[last_position]
			if position != last_position:
				node.duplicates[position] = last_element
				self._duplicate_pos[last_element] = position
			node.duplicates.pop()
			del self._duplicate_pos[idx]
		del self._elem_to_node[idx]

	def _remove_child(self, parent_idx: u32, child_idx: u32) -> None:
		children = self._nodes[parent_idx].children
		for i in range(len(children)):
			if children[i] == child_idx:
				children[i : i + 1] = []
				return

	def _removal_cover_sets(
		self, element_id: u32, min_level: int
	) -> tuple[list[tuple[int, list[u32]]], int]:
		root = self._nodes[self._root_idx]
		top_level = 0
		for i in range(len(root.children)):
			top_level = max(top_level, int(self._nodes[root.children[i]].level) + 1)
		top_level = min(top_level, MAX_LEVEL - 1)
		sets: list[tuple[int, list[u32]]] = []
		candidates: list[u32] = [self._root_idx]
		for level in range(top_level, min_level - 1, -1):
			sets.append((level, candidates))
			if level == min_level:
				break
			expanded = self._children_at_level(candidates, level - 1)
			candidates = [
				x
				for x in expanded
				if float(self._distance(element_id, self._nodes[x].element_id))
				<= self._radius(level)
			]
		return sets, top_level

	def _adopt_orphan(
		self,
		orphan_idx: u32,
		removed_node_idx: u32,
		removed_id: u32,
		cover_sets: list[tuple[int, list[u32]]],
		top_level: int,
	) -> None:
		parent_level = int(self._nodes[orphan_idx].level) + 1
		while True:
			if parent_level > top_level:
				candidates = [self._root_idx]
			else:
				candidates = cover_sets[top_level - parent_level][1]
			nearest_idx = NO_PARENT
			nearest_dist = float('inf')
			for candidate_idx in candidates:
				if candidate_idx == removed_node_idx:
					continue
				dist = float(
					self._distance(
						self._nodes[orphan_idx].element_id,
						self._nodes[candidate_idx].element_id,
					)
				)
				if dist <= self._radius(parent_level) and dist < nearest_dist:
					nearest_idx = candidate_idx
					nearest_dist = dist
			if nearest_idx != NO_PARENT:
				self._nodes[orphan_idx].parent = nearest_idx
				self._nodes[nearest_idx].children.append(orphan_idx)
				return

			self._set_node_level(orphan_idx, parent_level)
			for level, level_candidates in cover_sets:
				if level > parent_level:
					continue
				if float(
					self._distance(removed_id, self._nodes[orphan_idx].element_id)
				) <= self._radius(level + 1):
					if orphan_idx not in level_candidates:
						level_candidates.append(orphan_idx)
				else:
					break
			parent_level += 1

	def _find_node_by_id(self, element_id: u32) -> u32:
		"""Find node index with given element ID"""
		if self._root_idx == NO_PARENT:
			return NO_PARENT

		stack: list[u32] = [self._root_idx]
		while len(stack) > 0:
			node_idx = stack.pop()
			if node_idx in self._free_nodes:
				continue
			node = self._nodes[node_idx]
			if node.element_id == element_id:
				return node_idx
			for i in range(len(node.children)):
				stack.append(node.children[i])

		return NO_PARENT

	def _max_descendant_dist(self, level: int) -> float:
		"""
		Upper bound on distance from a node at `level` to any descendant.

		Each ancestor at level l covers its child within base^l. Summing
		the geometric series from level down gives base^(level+1)/(base-1).
		"""
		return self._base ** (level + 1) / (self._base - 1)

	def knn(
		self, v: np.ndarray[tuple[S], np.dtype[T]], k: int
	) -> typing.Iterator[VecDBElement[T, S, V, float]]:
		"""Find k nearest neighbors using cover tree with pruning"""
		self._do_init()

		if self._root_idx == NO_PARENT or k <= 0:
			return

		import heapq

		# Max-heap of size k tracking the best candidates (neg_dist, element_id)
		best: list[tuple[float, u32]] = []

		def best_kth_dist() -> float:
			if len(best) < k:
				return float('inf')
			return -best[0][0]

		# DFS with pruning; stack entries: (node_idx, dist_to_query)
		root_node = self._nodes[self._root_idx]
		root_dist = float(self._distance_to_point(root_node.element_id, v))
		stack: list[tuple[u32, float]] = [(self._root_idx, root_dist)]

		while len(stack) > 0:
			node_idx, node_dist = stack.pop()
			if node_idx in self._free_nodes:
				continue
			node = self._nodes[node_idx]

			# Add every database element represented by this metric point
			element_ids = [node.element_id]
			element_ids.extend(node.duplicates[i] for i in range(len(node.duplicates)))
			if np.isfinite(node_dist):
				for element_id in element_ids:
					if len(best) < k:
						heapq.heappush(best, (-node_dist, element_id))
					elif node_dist < -best[0][0]:
						heapq.heapreplace(best, (-node_dist, element_id))

			# Collect children with distances, then sort farthest-first
			# so DFS pops the closest child first (better pruning)
			children_with_dist: list[tuple[float, u32]] = []
			for i in range(len(node.children)):
				child_idx = node.children[i]
				child_node = self._nodes[child_idx]
				child_dist = float(self._distance_to_point(child_node.element_id, v))
				# Prune: closest possible descendant is child_dist - max_descendant_dist
				mdd = self._max_descendant_dist(int(child_node.level))
				if child_dist - mdd <= best_kth_dist():
					children_with_dist.append((child_dist, child_idx))

			# Sort descending so closest is popped first from stack
			children_with_dist.sort(key=lambda x: -x[0])
			for child_dist, child_idx in children_with_dist:
				stack.append((child_idx, child_dist))

		# Yield results sorted by distance
		results = sorted((-d, eid) for d, eid in best)
		for dist, eid in results:
			yield VecDBElement(self, eid, dist)

	def __iter__(self):
		self._do_init()

		for i in range(len(self._keys)):
			if i in self._free_idx:
				continue
			yield VecDBElement(self, i, None)
