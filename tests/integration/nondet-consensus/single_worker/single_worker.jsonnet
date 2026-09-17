local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{
	tags: util.features([['nondet', 'consensus', 'validator']], 'stable') + ['python'],
	entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py') {
		allow_two_workers: false,
	}]),
}
