// A read larger than the host's per-reply cap is served across several host
// reads, each landing at its own offset
local msg = import 'templates/message.json';
local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{
	tags: util.features([['storage']], 'stable') + ['python'],
	entry: util.addPaths([
		simple.run('${jsonnetDir}/${fileBaseName}.py') {
			next: [{
				vars: {},
				code: null,
				message: msg,
				calldata: |||
					{"": "read", "args": []}
				|||,
			}],
		},
	]),
}
