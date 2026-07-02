local deploy_then = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';

// A non-zero node genPerTimeUnit (7) that the balance-funded path must ignore in
// favour of the guest cap. gas_data replaces DEFAULT_GAS_DATA wholesale, so all
// required node fields are restated here.
local gasData = {
	storageUnitPrice: '1',
	receiptGasPerByte: '1',
	gasPerChangedSlot: '1',
	intrinsicGas: '0',
	bootloaderOverhead: '0',
	fixedProposeReceiptGas: '0',
	fixedMessageRevealGas: '0',
	genPerTimeUnit: '7',
	minTimeUnitsPerPhase: '0',
	messageBudgetFloor: '0',
};

// Fund the emitting contract so `value + metered_fee` clears the balance check.
local extra = {
	'balances': {
		'AQAAAAAAAAAAAAAAAAAAAAAAAAA=': 1000000,
	},
	gas_data: gasData,
};
local base = deploy_then.run('${jsonnetDir}/${fileBaseName}.py', 'do_emit');
{tags: util.features([['message'], ['fees']], 'stable'),
	entry: util.addPaths([base + extra + {next: [base.next[0] + extra]}])}
