local simple = import 'templates/simple.jsonnet';
local s = simple.run('${jsonnetDir}/code.py');
local util = import 'templates/util.jsonnet';
{tags: util.features([['storage', 'lock']], 'stable') + ['python'],
	entry: util.addPaths([util.chain([
	s {
		"calldata": |||
			{
				"args": [[], True],
			}
		|||,
		message: super.message + {
			"is_init": true,
		},
	},
	s {
		code: null,
		"calldata": |||
			{
				"": "nop",
			}
		|||
	}
])])}
