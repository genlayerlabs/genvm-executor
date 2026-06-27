local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['hello-world'], ['nondet']], 'stable'),
	entry: util.addPaths([simple_deploy.run('${jsonnetDir}/${fileBaseName}.py') {
	message+: {
		datetime: "2025-07-29T19:34:20+09:00",
	},
}])}
