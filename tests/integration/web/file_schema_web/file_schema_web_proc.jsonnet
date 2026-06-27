local simple = import 'templates/simple.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['web', 'render'], ['nondet']], 'semi-stable'),
	entry: util.addPaths([simple.run('${jsonnetDir}/file_schema_web.py')  + {
	"calldata": '{ "args": ["file:///proc/self/maps"] }',
	"message"+: {
		"is_init": true,
	},
}])}
