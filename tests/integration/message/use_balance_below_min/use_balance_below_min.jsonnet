local deploy_then = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';

// Pin both phase minima (30) above the emitted message's timeunits (5).
// gas_data replaces DEFAULT_GAS_DATA wholesale, so all required node fields are
// restated here (kept minimal/deterministic, matching DEFAULT_GAS_DATA).
local gasData = {
	storageUnitPrice: '1',
	receiptGasPerByte: '1',
	gasPerChangedSlot: '1',
	intrinsicGas: '0',
	bootloaderOverhead: '0',
	fixedProposeReceiptGas: '0',
	fixedMessageRevealGas: '0',
	overlaySplitBps: '0',
	receiptWrapperBytes: '1024',
	genPerTimeUnit: '0',
	minProposeTimeout: '30',
	maxProposeTimeout: '340282366920938463463374607431768211455',
	minCommitTimeout: '30',
	maxCommitTimeout: '340282366920938463463374607431768211455',
};

// Ample balance so the rejection isolates the phase bounds, not the balance.
local extra = {
	'balances': {
		'AQAAAAAAAAAAAAAAAAAAAAAAAAA=': 1000000,
	},
	gas_data: gasData,
};
local base = deploy_then.run('${jsonnetDir}/${fileBaseName}.py', 'do_emit');
{tags: util.features([['message', 'send'], ['balance'], ['fees', 'balance']], 'stable') + ['python'],
	entry: util.addPaths([base + extra + {next: [base.next[0] + extra]}])}
