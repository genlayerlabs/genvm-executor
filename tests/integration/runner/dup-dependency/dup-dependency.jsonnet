local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{
	tags: util.features([['runner', 'dependency']], 'stable') + ['python'],
	prepare: '${jsonnetDir}/dup-dependency-prepare.py',
	entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py')])
}
