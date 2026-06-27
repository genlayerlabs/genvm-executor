local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['permission'], ['message', 'external']], 'stable'),
	entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py', 'priv')])}
