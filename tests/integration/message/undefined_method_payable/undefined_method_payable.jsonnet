local simple = import 'templates/simple_deploy.jsonnet';
local msg = import 'templates/message.json';
local s = simple.run('${jsonnetDir}/${fileBaseName}.py');
local util = import 'templates/util.jsonnet';
{tags: util.features([['message', 'payable'], ['balance']], 'stable'),
	entry: util.addPaths([util.chain([
	s {
		message: s.message {
			"value": 100,
		}
	},
	s {
		code: null,
		message: msg,
		"calldata": |||
			{
				"": "main",
				"args": [],
			}
		|||,
	},
	s {
		code: null,
		message: msg {
			"value": 100,
		},
		"calldata": |||
			{}
		|||,
	},
])])}
