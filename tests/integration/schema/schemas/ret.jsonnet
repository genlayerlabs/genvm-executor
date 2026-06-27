local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['schema', 'prim'], ['balance'], ['message']], 'stable'),
	entry: util.addPaths([simple.run('${jsonnetDir}/trivial.py', '#get-schema')])}
