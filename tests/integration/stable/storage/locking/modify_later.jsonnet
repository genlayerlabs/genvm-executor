local simple = import 'templates/simple.jsonnet';
local s = simple.run('${jsonnetDir}/code.py');
local util = import 'templates/util.jsonnet';
{tags: ["feature-storage-lock", "python"], entry: util.addPaths([util.chain([
	s {
		"calldata": |||
			{
				"args": [[Address(b'\x02\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00')], False],
			}
		|||,
	},
	s {
		"calldata": |||
			{
				"": "try_modify",
			}
		|||,
		code: null,
	},
	s {
		"calldata": |||
			{
				"": "nop",
			}
		|||,
		code: null,
	}
])])}
