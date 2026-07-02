local deploy_then = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
// Balance far below the metered fee, so `value + metered_fee` overflows it.
local bal = {
	'balances': {
		'AQAAAAAAAAAAAAAAAAAAAAAAAAA=': 100,
	},
};
local base = deploy_then.run('${jsonnetDir}/${fileBaseName}.py', 'do_emit');
{tags: util.features([['message'], ['balance'], ['fees', 'balance']], 'stable'),
	entry: util.addPaths([base + bal + {next: [base.next[0] + bal]}])}
