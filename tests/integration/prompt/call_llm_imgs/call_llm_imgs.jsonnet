local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{
	tags: util.features([['prompt', 'vision'], ['prompt', 'comparative'], ['nondet']], 'unstable'),
	entry: util.addPaths([
		simple_deploy.run('${jsonnetDir}/${fileBaseName}.py') { stable_hash: false }
	])
}
