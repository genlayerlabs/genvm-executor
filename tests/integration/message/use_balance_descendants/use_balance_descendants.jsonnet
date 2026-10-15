local deployThen = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';

local balance = {
	balances: {
		'AQAAAAAAAAAAAAAAAAAAAAAAAAA=': 10000000,
	},
};
// gas_data replaces DEFAULT_GAS_DATA wholesale
local gasData = {
	storageUnitPrice: '1',
	receiptGasPerByte: '1',
	gasPerChangedSlot: '1',
	intrinsicGas: '0',
	bootloaderOverhead: '0',
	fixedProposeReceiptGas: '0',
	fixedMessageRevealGas: '0',
	lockedReceiptGasPrice: '1',
	overlaySplitBps: '0',
	receiptWrapperBytes: '1024',
	minProposeTimeout: '1',
	maxProposeTimeout: '340282366920938463463374607431768211455',
	minCommitTimeout: '1',
	maxCommitTimeout: '340282366920938463463374607431768211455',
	genPerTimeUnit: '0',
	messageBudgetFloor: '0',
};
local base = deployThen.run('${jsonnetDir}/${fileBaseName}.py', 'open');
local write(method) = base.next[0] + balance + {
	slug: method,
	calldata: std.manifestJsonEx({'': method}, '    '),
};
local externalFloor(slug, floor) = write('external_floor') + {
	slug: slug,
	gas_data: gasData + {minExternalGasLimit: floor},
};
local case = base + balance + {
	next: [
		write('open'),
		write('closed_zero'),
		write('pinned_nested'),
		write('depth_five'),
		write('depth_six'),
		write('invalid_tree'),
		write('external_budget'),
		externalFloor('external_at_floor', '20'),
		externalFloor('external_below_floor', '21'),
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
