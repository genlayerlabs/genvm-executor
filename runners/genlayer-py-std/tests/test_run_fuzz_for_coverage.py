import itertools
from pathlib import Path

src_dir = Path(__file__).parent.parent.joinpath('fuzz', 'src')

for test in sorted(src_dir.glob('*.py')):
	name = test.stem
	print(test, name)

	src_py = test.read_text()
	new_globs = {}  # globals().copy()
	new_globs['__file__'] = str(test)
	src_compile = compile(src_py, str(test), 'exec')
	exec(src_compile, new_globs)
	fun = new_globs[name]

	inputs_dir = src_dir.parent.joinpath('inputs')
	for testcase in itertools.chain(
		inputs_dir.joinpath(name).glob('**/*'),
		inputs_dir.joinpath(f'{name}-curated').glob('*'),
	):
		if (
			not testcase.is_file()
			or testcase.name == 'README.txt'
			or testcase.name.startswith('.')
		):
			continue

		def cur_test(testcase=testcase):
			fun(testcase.read_bytes())

		testname = 'test_' + name + '_' + testcase.name.replace('.', '_').replace('-', '_')

		cur_test.__name__ = testname

		globals()[testname] = cur_test
