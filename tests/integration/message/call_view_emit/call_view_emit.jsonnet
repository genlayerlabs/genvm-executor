local simple = import 'templates/two.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['message', 'view'], ['event'], ['message', 'send']], 'stable'),
	entry: util.addPaths([simple.run('${jsonnetDir}/call_view_emit_from.py', '${jsonnetDir}/call_view_emit_to.py',
	|||
		{
			"": "main",
			"args": [Address(toAddr)]
		}
	|||
)])}
