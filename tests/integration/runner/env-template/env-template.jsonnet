local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['runner'], ['wasi', 'environ']], 'stable'),
	entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py')])}
