local deploy_then = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';

// Pin a per-round execution-budget floor (2000) above the emitted message's
// executionBudgetPerRound (1024). gas_data replaces DEFAULT_GAS_DATA wholesale,
// so all required node fields are restated here.
local gasData = {
	storageUnitPrice: '1',
	receiptGasPerByte: '1',
	gasPerChangedSlot: '1',
	intrinsicGas: '0',
	bootloaderOverhead: '0',
	fixedProposeReceiptGas: '0',
	fixedMessageRevealGas: '0',
	genPerTimeUnit: '0',
	minTimeUnitsPerPhase: '0',
	messageBudgetFloor: '2000',
};

// Ample balance so the rejection isolates the budget floor, not the balance.
local extra = {
	'balances': {
		'AQAAAAAAAAAAAAAAAAAAAAAAAAA=': 1000000,
	},
	gas_data: gasData,
};
local base = deploy_then.run('${jsonnetDir}/${fileBaseName}.py', 'do_emit');
{tags: util.features([['message'], ['fees']], 'stable'),
	entry: util.addPaths([base + extra + {next: [base.next[0] + extra]}])}
