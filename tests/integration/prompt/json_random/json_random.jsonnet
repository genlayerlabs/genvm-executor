local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{
	tags: util.features([['prompt', 'json'], ['nondet']], 'unstable') + ['needs-llm', 'needs-web', 'python'],
	entry: util.addPaths([
		simple_deploy.run('${jsonnetDir}/${fileBaseName}.py') { stable_hash: false },
	]),
}
