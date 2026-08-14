local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ['python', 'feature-runner-malformed'], entry: util.addPaths([simple.run('${jsonnetDir}/malformed_runner.py') {
	"calldata": |||
		{
		}
	|||
}])}
