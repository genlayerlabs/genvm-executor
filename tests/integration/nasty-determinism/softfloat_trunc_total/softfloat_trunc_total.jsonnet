local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['nasty-determinism', 'float'], ['runner', 'dependency']], 'stable') + ['wasm'],
	entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.wat')])}
