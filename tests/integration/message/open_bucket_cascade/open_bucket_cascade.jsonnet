local simple = import 'templates/simple_deploy.jsonnet';
local msg = import 'templates/message.json';
local util = import 'templates/util.jsonnet';
local s = simple.run('${jsonnetDir}/${fileBaseName}.py');
// Default open-pool wildcards plus the cascade a node sends for an open bucket
local extra = {
	'balances': {
		'AQAAAAAAAAAAAAAAAAAAAAAAAAA=': 1000000,
	},
	'message_fee_cascade': {
		'min_primary_fee': 100000,
		'budget_remaining': 1000000,
		'external_fee_reserve': 0,
	},
};
{tags: util.features([['message', 'send'], ['balance'], ['fees', 'balance']], 'stable') + ['python'],
	entry: util.addPaths([util.chain([
		s + extra,
		s + extra + {
			'code': null,
			'message': msg,
			'calldata': std.manifestJsonEx({'': 'emit', 'args': []}, '    '),
		},
	])])}
