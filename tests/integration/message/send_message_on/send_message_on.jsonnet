local simple = import 'templates/simple_deploy.jsonnet';
local msg = import 'templates/message.json';
local s = simple.run('${jsonnetDir}/${fileBaseName}.py');
local util = import 'templates/util.jsonnet';
{tags: util.features([['message', 'send']], 'stable') + ['python'],
	entry: util.addPaths([util.chain([
	s {
	},
	s {
		code: null,
		message: msg,
		"calldata": |||
			{
				"": "main",
				"args": ["accepted"],
			}
		|||
	},
	s {
		code: null,
		message: msg,
		"calldata": |||
			{
				"": "main",
				"args": ["random"],
			}
		|||
	},
])])}
