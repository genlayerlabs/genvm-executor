local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{
	tags: ['python', 'feature-runner-lock'],
	prepare: '${jsonnetDir}/prepare.py',
	entry: util.addPaths([simple.run('${jsonnetDir}/contract.zip') {stable_hash: false}])
}
