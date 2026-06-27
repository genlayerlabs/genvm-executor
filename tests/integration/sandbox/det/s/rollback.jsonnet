local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['user-error'], ['sandbox']], 'stable'),
	entry: util.addPaths([simple.run('${jsonnetDir}/../code.py', 'main', ["gl.vm.UserError.immediate('RB')"])])}
