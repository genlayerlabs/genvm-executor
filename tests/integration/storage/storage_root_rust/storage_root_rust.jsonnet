local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{
	tags: util.features([['storage', 'root']], 'stable') + ['rust', 'wasm', 'slow'],
	prepare: '${jsonnetDir}/prepare.py',
	entry: util.addPaths([
		simple.run('${jsonnetDir}/storage_root.wasm') {
			"calldata": |||
				{}
			|||,
			stable_hash: false,
		}])
}
