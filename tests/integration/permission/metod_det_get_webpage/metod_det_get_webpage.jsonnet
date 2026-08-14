local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['permission', 'module'], ['web', 'render'], ['nondet']], 'stable') + ['needs-web', 'python'],
	entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py', 'det_viol')])}
