local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['web', 'render'], ['nondet']], 'unstable') + ['needs-web', 'python', 'slow'],
	entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py') {
		deadline: 120,
		stable_hash: false,
	}])}
