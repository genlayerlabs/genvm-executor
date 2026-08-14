local simple = import 'templates/simple_deploy.jsonnet';
local msg = import 'templates/message.json';
local util = import 'templates/util.jsonnet';
local s = simple.run('${jsonnetDir}/${fileBaseName}.py');
// Empty allocation tree (no matching node) + a funded balance for use_balance.
local extra = {
	'balances': {
		'AQAAAAAAAAAAAAAAAAAAAAAAAAA=': 1000000,
	},
	'message_fee_allocation': [],
};
{tags: util.features([['message', 'send'], ['balance'], ['fees', 'balance']], 'stable') + ['python'],
	entry: util.addPaths([util.chain([
		s + extra,
		s + extra + {
			'code': null,
			'message': msg,
			'calldata': std.manifestJsonEx({'': 'emit_normal', 'args': []}, '    '),
		},
		s + extra + {
			'code': null,
			'message': msg,
			'calldata': std.manifestJsonEx({'': 'emit_balance', 'args': []}, '    '),
		},
	])])}
