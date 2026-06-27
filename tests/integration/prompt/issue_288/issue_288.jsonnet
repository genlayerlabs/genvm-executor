local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['prompt', 'json'], ['nondet']], 'semi-stable'),
	entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py') {
	stable_hash: false,
	modes: 'l',
}])}
