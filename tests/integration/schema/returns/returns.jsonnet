local simple_deploy = import 'templates/simple_deploy.jsonnet';
local msg = import 'templates/message.json';
local util = import 'templates/util.jsonnet';
local call(idx) = {
	vars: {},
	code: null,
	message: msg,
	calldata: std.manifestJsonEx({ '': 'main', args: [idx] }, '    '),
};
{
	tags: util.features([['schema', 'primitive'], ['schema', 'complex']], 'stable') + ['python'],
	// deploy once, then call main(idx) for each idx on top of the deployed contract
	entry: util.addPaths([simple_deploy.run('${jsonnetDir}/${fileBaseName}.py') {
		next: [call(idx) for idx in std.range(0, 9)],
	}]),
}
