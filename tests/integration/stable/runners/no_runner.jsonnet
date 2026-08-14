local simple = import 'templates/simple.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ['python', 'feature-runner-malformed'], entry: util.addPaths([simple.run('${jsonnetDir}/no_runner.py') {
	"calldata": |||
		{
		}
	|||
}])}
