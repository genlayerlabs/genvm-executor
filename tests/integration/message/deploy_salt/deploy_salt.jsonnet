local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['message', 'deploy'], ['message', 'salt']], 'stable'),
	entry: util.addPaths([simple_deploy.run('${jsonnetDir}/${fileBaseName}.py')])}
