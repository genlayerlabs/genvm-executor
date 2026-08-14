local simple = import 'templates/simple.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['web', 'render'], ['nondet']], 'semi-stable') + ['python'],
	entry: util.addPaths([simple.run('${jsonnetDir}/file_schema_web.py')  + {
	"calldata": '{ "args": ["file:///usr/bin/env"] }',
	"message"+: {
		"is_init": true,
	},
}])}
