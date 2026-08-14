local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{
	tags: ['python', 'feature-runner-multi-file', 'feature-runner-zip'],
	prepare: '${jsonnetDir}/prepare.py',
	entry: util.addPaths([simple_deploy.run('${jsonnetDir}/contract.zip') {stable_hash: false}])
}
