local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['prompt', 'text'], ['nasty-determinism'], ['nondet']], 'stable'),
	entry: util.addPaths([simple.run('${jsonnetDir}/simple.py', 'main', [true])])}
