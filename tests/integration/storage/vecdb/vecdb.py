# {
#   "Seq": [
#     { "Depends": "py-lib-genlayer-embeddings:test" },
#     { "Depends": "py-genlayer:test" }
#   ]
# }


import typing

import genlayer as gl
import genlayer_embeddings as gle
import numpy as np


class Contract(gl.contract.Contract):
	x: gle.VecDB[np.float32, typing.Literal[5], str, gle.EuclideanDistance]

	def __init__(self):
		self.x.insert(np.array([1, 2, 3, 4, 5], dtype=np.float32), '123')
		print(list(self.x.knn(np.array([1, 1, 1, 1, 1], dtype=np.float32), 1)))
