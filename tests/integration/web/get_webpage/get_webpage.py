# { "Depends": "py-genlayer:test" }
from html.parser import HTMLParser

import genlayer as gl

fcf = set(['script', 'iframe'])


class ScriptRemover(HTMLParser):
	result: list[str]

	def __init__(self):
		super().__init__()
		self.result = []
		self.in_script = False

	def handle_starttag(self, tag, attrs):
		if tag.lower() in fcf:
			self.in_script = True
		elif not self.in_script:
			txt = self.get_starttag_text()
			if txt is not None:
				self.result.append(txt)

	def handle_endtag(self, tag):
		if tag.lower() in fcf:
			self.in_script = False
		elif not self.in_script:
			self.result.append(f'</{tag}>')

	def handle_data(self, data):
		if not self.in_script:
			self.result.append(data)


class Contract(gl.contract.Contract):
	@gl.public.write
	def main(self, mode: str):
		def run() -> str:
			assert mode in ('html', 'text'), f'Invalid mode value: {mode}'
			res = gl.nondet.web.render(
				'https://test-server.genlayer.com/static/genvm/hello.html', mode=mode
			)
			if mode == 'html':
				parser = ScriptRemover()
				parser.feed(res)
				res = ''.join(parser.result)
			return res

		print(gl.eq_principle.strict_eq(run).strip())
