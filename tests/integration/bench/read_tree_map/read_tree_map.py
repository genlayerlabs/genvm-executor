# { "Depends": "py-genlayer:test" }

import random
from typing import Iterable

import genlayer as gl
from genlayer import u8, u32


@gl.storage.allow
class TrieNode[K: gl.storage.Comparable, T]:
	children: gl.storage.TreeMap[K, u32]
	value: T
	has_value: bool


@gl.storage.allow
class Trie[K: gl.storage.Comparable, T]:
	nodes: gl.storage.DynArray[TrieNode[K, T]]

	def __init__(self):
		_root = self.nodes.append_new_get()

	def insert(self, its: Iterable[K], value: T) -> None:
		node_index = 0
		for k in its:
			node = self.nodes[node_index]
			child_index = node.children.get(k)
			if child_index is None:
				child_index = len(self.nodes)
				node.children[k] = child_index
				self.nodes.append_new_get()
			node_index = child_index

		node = self.nodes[node_index]
		node.value = value
		node.has_value = True

	def get(self, its: Iterable[K]) -> T | None:
		node_index = 0
		for k in its:
			node = self.nodes[node_index]
			child_index = node.children.get(k)
			if child_index is None:
				return None
			node_index = child_index

		node = self.nodes[node_index]
		if node.has_value:
			return node.value
		else:
			return None


def str_to_keys(s: str) -> list[u8]:
	return [ord(c) for c in s]


rand_str_chars = 'abcdefghijklmn'


class Contract(gl.contract.Contract):
	trie: Trie[u8, u8]

	def __init__(self):
		self.trie.__init__()
		rnd = random.Random(0)
		for i in range(100):
			s = ''.join(rnd.choice(rand_str_chars) for _ in range(4))
			self.trie.insert(str_to_keys(s), i)

	@gl.public.write
	def bench(self) -> None:
		sum = 0
		rnd = random.Random(1)
		for i in range(5_000):
			s = ''.join(rnd.choice(rand_str_chars) for _ in range(4))
			x = self.trie.get(str_to_keys(s))
			if x is not None:
				sum += x
		print(sum)
