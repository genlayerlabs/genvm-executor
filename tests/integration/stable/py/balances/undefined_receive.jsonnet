local simple = import 'templates/simple_deploy.jsonnet';
local msg = import 'templates/message.json';
local s = simple.run('${jsonnetDir}/undefined_receive.py');
local util = import 'templates/util.jsonnet';
{tags: ["feature-balance", "feature-message-external", "feature-message-payable", "python"], entry: util.addPaths([util.chain([
	s {
		"calldata": |||
			{
				"": "main",
				"args": [],
			}
		|||,
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
