local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{
	tags: util.features([['runner', 'dependency']], 'stable'),
	prepare: '${jsonnetDir}/prepare.py',
	entry: util.addPaths([
		simple.run('${jsonnetDir}/contract.zip') {
			stable_hash: false,
			runner_load_asserts: [
				{match: {}, count: 1},
				{match: {status: 'charged'}, count: 1},
				{match: {status: 'cached'}, count: 0},
			],
		},
	]),
}
