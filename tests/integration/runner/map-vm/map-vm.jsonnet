local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{
	tags: util.features([['runner', 'map-vm']], 'stable') + ['python'],
	prepare: '${jsonnetDir}/prepare.py',
	entry: util.addPaths([simple_deploy.run('${jsonnetDir}/contract.zip') {stable_hash: false}])
}
