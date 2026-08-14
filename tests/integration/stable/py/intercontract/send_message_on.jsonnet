local simple = import 'templates/simple_deploy.jsonnet';
local msg = import 'templates/message.json';
local s = simple.run('${jsonnetDir}/send_message_on.py');
local util = import 'templates/util.jsonnet';
{tags: ["feature-message-send", "python"], entry: util.addPaths([util.chain([
	s {
		"calldata": |||
			{
				"": "main",
				"args": ["finalized"],
			}
		|||
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
