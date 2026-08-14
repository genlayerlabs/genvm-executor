local simple_deploy = import 'templates/simple_deploy.jsonnet';
local msg = import 'templates/message.json';
local util = import 'templates/util.jsonnet';
{tags: util.features([['message', 'view'], ['message', 'external', 'view'], ['message', 'deploy']], 'stable') + ['python'],
	entry: util.addPaths([simple_deploy.run('${jsonnetDir}/${fileBaseName}.py') {
		next: [{
			vars: {},
			code: null,
			message: msg,
			calldata: std.manifestJsonEx({ '': 'fib', args: [6] }, '    '),
		}],
	}])}
