# {
#   "Seq": [
#     { "Depends": "py-lib-word_piece_tokenizer:test" },
#     { "Depends": "py-genlayer:test" }
#   ]
# }

import genlayer as gl
import word_piece_tokenizer


class Contract(gl.contract.Contract):
	@gl.public.write
	def main(self, det: bool):
		tokenizer = word_piece_tokenizer.WordPieceTokenizer()

		for txt in ['##', '#hashtag', 'emoji 🚀🚀🚀']:
			print(f'{txt}: {tokenizer.tokenize(txt)}')
