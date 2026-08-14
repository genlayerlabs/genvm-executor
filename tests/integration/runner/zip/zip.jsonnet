local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{
	tags: util.features([['runner', 'zip']], 'stable') + ['python'],
	prepare: '${jsonnetDir}/prepare.py',
	entry: util.addPaths(util.mapGraph(function(e) e {stable_hash: false}, [simple.run('${jsonnetDir}/contract.zip')]))
}
