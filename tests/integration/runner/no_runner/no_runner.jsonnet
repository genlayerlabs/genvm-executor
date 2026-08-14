local simple = import 'templates/simple.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['runner', 'malformed']], 'stable') + ['python'],
	entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py') {
	"calldata": |||
		{
		}
	|||
}])}
