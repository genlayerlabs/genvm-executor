local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['sandbox', 'non-det']], 'stable'),
	entry: util.addPaths([simple.run('${jsonnetDir}/../code.py', 'main', ["exec(\"json.loads.__name__ = 'haha'\")"])])}
