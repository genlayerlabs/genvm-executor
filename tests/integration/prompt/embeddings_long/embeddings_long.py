# {
#   "Seq": [
#     { "Depends": "py-lib-genlayer-embeddings:test" },
#     { "Depends": "py-genlayer:test" }
#   ]
# }

import genlayer as gl
import genlayer_embeddings as gle

# Exactly 100 characters.
LONG_TEXT = 'GenLayer smart contracts can compute sentence embeddings for fairly long input strings inside GenVM.'


class Contract(gl.contract.Contract):
	@gl.public.write
	def main(self, det: bool):
		def nd_block():
			embeddings_generator = gle.SentenceTransformer('all-MiniLM-L6-v2')
			real = embeddings_generator(LONG_TEXT)
			print(real.sum())

		if det:
			nd_block()
		else:
			gl.eq_principle.strict_eq(nd_block)
