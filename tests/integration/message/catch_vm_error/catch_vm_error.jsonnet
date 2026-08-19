local simple = import 'templates/two.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['message', 'view'], ['message', 'external', 'view'], ['user-error']], 'stable') + ['python'],
	entry: util.addPaths([simple.run('${jsonnetDir}/catch_vm_error_from.py', '${jsonnetDir}/catch_vm_error_to.py',
	|||
		{
			"": "main",
			"args": [Address(toAddr)]
		}
	|||
)])}
