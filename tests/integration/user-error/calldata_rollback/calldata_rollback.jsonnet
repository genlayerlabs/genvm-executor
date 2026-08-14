local simple = import 'templates/two.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['user-error'], ['message', 'external', 'view']], 'stable') + ['python'],
	entry: util.addPaths([simple.run('${jsonnetDir}/calldata_rollback_from.py', '${jsonnetDir}/calldata_rollback_to.py', |||
		{
			"": "main",
			"args": [Address(toAddr)]
		}
	|||
)])}
