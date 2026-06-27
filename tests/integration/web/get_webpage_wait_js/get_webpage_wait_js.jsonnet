local simple = import 'templates/simple_deploy.jsonnet';
local msg = import 'templates/message.json';
local s = simple.run('${jsonnetDir}/${fileBaseName}.py');
local util = import 'templates/util.jsonnet';
{tags: util.features([['web', 'render'], ['nondet']], 'unstable'),
	entry: util.addPaths([util.chain([
	s {
		"calldata": |||
			{
				"": "main",
				"args": ["15s"]
			}
		|||,
		deadline: 60,
		stable_hash: false,
	},
	s {
		code: null,
		message: msg,
		"calldata": |||
			{
				"": "main",
				"args": ["0ms"]
			}
		|||,
		deadline: 60,
		stable_hash: false,
	}
])])}
