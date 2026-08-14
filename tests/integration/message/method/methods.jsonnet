local simple_deploy = import 'templates/simple_deploy.jsonnet';
local msg = import 'templates/message.json';
local util = import 'templates/util.jsonnet';
local call(method) = {
	vars: {},
	code: null,
	message: msg,
	calldata: std.manifestJsonEx({ '': method, args: [] }, '    '),
};
{
	tags: util.features([['message', 'external'], ['message', 'external', 'view'], ['message', 'view'], ['schema'], ['user-error']], 'stable') + ['python'],
	// deploy once, then fan out independent calls on top of the deployed contract
	entry: util.addPaths([simple_deploy.run('${jsonnetDir}/${fileBaseName}.py') {
		next: [
			call('pub'),
			call('retn'),
			call('retn_view'),
			call('rback'),
		],
	}]),
}
