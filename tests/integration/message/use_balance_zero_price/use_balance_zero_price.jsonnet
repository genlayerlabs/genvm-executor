local deploy_then = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
local base = deploy_then.run('${jsonnetDir}/${fileBaseName}.py', 'do_emit');
{tags: util.features([['message', 'send'], ['balance'], ['fees', 'balance']], 'stable') + ['python'],
	entry: util.addPaths([base])}
