local simple_deploy_then_write = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['storage', 'root']], 'stable') + ['python'],
	entry: util.addPaths([simple_deploy_then_write.run('${jsonnetDir}/${fileBaseName}.py', 'foo')])}
