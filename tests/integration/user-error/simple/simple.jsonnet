local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['user-error']], 'stable'),
	entry: util.addPaths([simple_deploy.run('${jsonnetDir}/${fileBaseName}.py')])}
