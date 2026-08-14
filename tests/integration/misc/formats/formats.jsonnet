local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['storage', 'dynamic-array'], ['storage', 'tree-map']], 'stable') + ['python'],
	entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py')])}
