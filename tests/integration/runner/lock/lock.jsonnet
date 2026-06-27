local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{
	tags: util.features([['runner', 'lock']], 'stable'),
	prepare: '${jsonnetDir}/prepare.py',
	entry: util.addPaths([simple.run('${jsonnetDir}/contract.zip') {stable_hash: false}])
}
