local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['wasi', 'clock']], 'stable') + ['python'],
	entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py')])}
