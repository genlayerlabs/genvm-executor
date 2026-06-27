local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{
	tags: util.features([['web', 'render']], 'unstable'),
	prepare: '${jsonnetDir}/prepare.py',
	entry: util.addPaths([
		simple.run('${jsonnetDir}/fetch_webpage.wasm') {
			"calldata": |||
				{}
		|||,
		stable_hash: false,
	}])
}
