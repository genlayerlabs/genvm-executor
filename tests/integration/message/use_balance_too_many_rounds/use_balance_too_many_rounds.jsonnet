local deploy_then = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
// Ample balance so the rejection isolates the rounds guard, not the balance.
local bal = {
	'balances': {
		'AQAAAAAAAAAAAAAAAAAAAAAAAAA=': 1000000,
	},
};
local base = deploy_then.run('${jsonnetDir}/${fileBaseName}.py', 'do_emit');
{tags: util.features([['message'], ['fees']], 'stable'),
	entry: util.addPaths([base + bal + {next: [base.next[0] + bal]}])}
