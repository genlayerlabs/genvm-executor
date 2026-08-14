local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
local base = simple.run('${jsonnetDir}/${fileBaseName}.py');
{tags: util.features([['web', 'render', 'wait-js'], ['nondet']], 'unstable') + ['needs-web', 'needs-time', 'python', 'slow'],
	entry: util.addPaths([
	base {
		"calldata": |||
			{
				"args": [128]
			}
		|||,
		deadline: 60,
		stable_hash: false,
	},
	base {
		"calldata": |||
			{
				"args": [2048]
			}
		|||,
		deadline: 60,
		stable_hash: false,
	},
])}
