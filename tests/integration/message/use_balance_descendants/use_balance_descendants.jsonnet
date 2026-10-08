local deployThen = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';

local balance = {
	balances: {
		'AQAAAAAAAAAAAAAAAAAAAAAAAAA=': 10000000,
	},
};
local base = deployThen.run('${jsonnetDir}/${fileBaseName}.py', 'open');
local write(method) = base.next[0] + balance + {
	slug: method,
	calldata: std.manifestJsonEx({'': method}, '    '),
};
local case = base + balance + {
	next: [
		write('open'),
		write('closed_zero'),
		write('pinned_nested'),
		write('invalid_tree'),
		write('external_budget'),
		write('deploy_open'),
		write('nested_decided_underfunded'),
		write('duplicate_external_selector'),
		write('pinned_selector'),
	],
};
{
	tags: util.features(
		[['message', 'send'], ['balance'], ['fees', 'balance']],
		'stable'
	) + ['python'],
	entry: util.addPaths(util.mapGraph(function(entry) entry + {stable_hash: false}, [case])),
}
