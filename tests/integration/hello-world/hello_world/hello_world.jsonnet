local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['hello-world']], 'stable'),
	entry: util.addPaths([simple.run('${jsonnetDir}/hello_world_trivial.py')])}
