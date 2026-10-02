local deploy_then = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';

// Ample balance so the rejection isolates the zero declared budget
local extra = {
	'balances': {
		'AQAAAAAAAAAAAAAAAAAAAAAAAAA=': 1000000,
	},
};
local base = deploy_then.run('${jsonnetDir}/${fileBaseName}.py', 'do_emit');
{tags: util.features([['message', 'send'], ['balance'], ['fees', 'balance']], 'stable') + ['python'],
	entry: util.addPaths([base + extra + {next: [base.next[0] + extra]}])}
