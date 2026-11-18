// Calldata at the cap reaches the host, which reverts; one byte more is refused
// before the host is asked
local msg = import 'templates/message.json';
local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
local call(method) = {
	vars: {},
	code: null,
	message: msg,
	calldata: std.manifestJsonEx({'': method, args: []}, '    '),
};
{
	tags: util.features([['message', 'eth'], ['message', 'view']], 'stable') + ['python'],
	entry: util.addPaths([
		simple.run('${jsonnetDir}/${fileBaseName}.py') {
			next: [
				call('at_cap'),
				call('over_cap'),
			],
		},
	]),
}
