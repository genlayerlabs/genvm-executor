local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['message', 'send', 'eth'], ['message', 'eth'], ['balance']], 'stable') + ['python'],
	entry: util.addPaths([simple_deploy.run('${jsonnetDir}/${fileBaseName}.py') {
	message+: {
		"value": 100,
	}
}])}
