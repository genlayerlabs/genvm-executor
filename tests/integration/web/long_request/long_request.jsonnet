local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{
	tags: util.features([['web', 'render'], ['nondet']], 'unstable') + ['needs-web', 'needs-time', 'python', 'slow'],
	entry: util.addPaths([simple_deploy.run('${jsonnetDir}/${fileBaseName}.py') {
		"deadline": 5,
		stable_hash: false,
		modes: "l",
	}]),
}
