# { "Depends": "py-genlayer:test" }

import io
import re
import sys

import genlayer as gl
from PIL import Image

# 5x7 bitmaps of the letters this test renders, one string per glyph row.
FONT = {
	'A': ('01110', '10001', '10001', '11111', '10001', '10001', '10001'),
	'D': ('11110', '10001', '10001', '10001', '10001', '10001', '11110'),
	'E': ('11111', '10000', '10000', '11110', '10000', '10000', '11111'),
	'G': ('01110', '10001', '10000', '10111', '10001', '10001', '01111'),
	'L': ('10000', '10000', '10000', '10000', '10000', '10000', '11111'),
	'N': ('10001', '11001', '10101', '10011', '10001', '10001', '10001'),
	'P': ('11110', '10001', '10001', '11110', '10000', '10000', '10000'),
	'R': ('11110', '10001', '10001', '11110', '10100', '10010', '10001'),
	'T': ('11111', '00100', '00100', '00100', '00100', '00100', '00100'),
}

GLYPH_W, GLYPH_H = 5, 7
SCALE = 12  # font pixel -> image pixel
MARGIN = 12
GAP = 2  # blank font columns between glyphs


def render(word: str) -> bytes:
	"""
	Render `word` as a black-on-white PNG.
	"""
	glyphs = [FONT[c] for c in word]
	width = (len(glyphs) * GLYPH_W + (len(glyphs) - 1) * GAP) * SCALE + 2 * MARGIN
	height = GLYPH_H * SCALE + 2 * MARGIN

	pixels = bytearray(b'\xff' * (width * height))
	for i, glyph in enumerate(glyphs):
		for gy, line in enumerate(glyph):
			for gx, bit in enumerate(line):
				if bit != '1':
					continue
				x0 = MARGIN + (i * (GLYPH_W + GAP) + gx) * SCALE
				y0 = MARGIN + gy * SCALE
				for y in range(y0, y0 + SCALE):
					pixels[y * width + x0 : y * width + x0 + SCALE] = b'\x00' * SCALE

	buf = io.BytesIO()
	Image.frombytes('L', (width, height), bytes(pixels)).save(buf, format='PNG')
	return buf.getvalue()


first_im_data = render('GARDEN')
second_im_data = render('PLANET')


class Contract(gl.contract.Contract):
	def __init__(self):
		def run():
			res = gl.nondet.exec_prompt(
				'each image contains a single word; read both and respond with the two words in order, separated by a space, without any context',
				images=[first_im_data, second_im_data],
			)
			print(res, file=sys.stderr)
			# Lenient about surrounding prose, strict about which words appear and
			# in which order — that is what proves both images reached the model.
			words = re.findall(r'[a-z]+', res.lower())
			if 'garden' not in words or 'planet' not in words:
				return False
			return words.index('garden') < words.index('planet')

		res = gl.eq_principle.strict_eq(run)
		print(res)
