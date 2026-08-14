local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['prompt', 'comparative'], ['nondet']], 'semi-stable') + ['needs-llm', 'needs-web', 'python', 'slow'],
	entry: util.addPaths([simple_deploy.run('${jsonnetDir}/${fileBaseName}.py') {
	leader_nondet: [
		{
			"kind": "return",
			"value": "%0"
		}
	]
}])}
