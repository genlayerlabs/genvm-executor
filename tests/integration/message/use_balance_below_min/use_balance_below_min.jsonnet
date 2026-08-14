local deploy_then = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';

// Pin a per-phase timeunit floor (30) above the emitted message's timeunits (5).
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
	genPerTimeUnit: '0',
	minTimeUnitsPerPhase: '30',
};

// Ample balance so the rejection isolates the timeunit floor, not the balance.
local extra = {
	'balances': {
		'AQAAAAAAAAAAAAAAAAAAAAAAAAA=': 1000000,
	},
	gas_data: gasData,
};
local base = deploy_then.run('${jsonnetDir}/${fileBaseName}.py', 'do_emit');
{tags: util.features([['message', 'send'], ['balance'], ['fees', 'balance']], 'stable') + ['python'],
	entry: util.addPaths([base + extra + {next: [base.next[0] + extra]}])}
