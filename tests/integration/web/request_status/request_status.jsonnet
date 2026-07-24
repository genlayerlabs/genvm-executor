local simple = import 'templates/simple_deploy.jsonnet';
local msg = import 'templates/message.json';
local s = simple.run('${jsonnetDir}/${fileBaseName}.py');
local util = import 'templates/util.jsonnet';
{tags: util.features([['web', 'request'], ['nondet']], 'unstable'),
	entry: util.addPaths([util.chain([
	s {
		stable_hash: false,
	},
	s {
		code: null,
		message: msg,
		"calldata": |||
			{
				"": "main",
				"args": [404]
			}
		|||,
		stable_hash: false,
	},
])])}
