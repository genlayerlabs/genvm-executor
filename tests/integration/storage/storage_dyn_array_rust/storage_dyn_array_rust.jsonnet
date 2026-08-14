local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{
	tags: util.features([['storage', 'dynamic', 'array']], 'stable') + ['rust', 'wasm', 'slow'],
	prepare: '${jsonnetDir}/prepare.py',
	entry: util.addPaths([
		simple.run('${jsonnetDir}/storage_dyn_array.wasm') {
			"calldata": |||
				{}
			|||,
			stable_hash: false,
		}])
}
