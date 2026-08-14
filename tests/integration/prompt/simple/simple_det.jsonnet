local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['prompt', 'embedding'], ['nasty-determinism', 'float']], 'stable') + ['python', 'slow'],
	entry: util.addPaths([simple.run('${jsonnetDir}/simple.py', 'main', [true])])}
