local deploy_then = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';

// Same floor as use_balance_budget_too_low (2000), but the emitted message
// declares a zero executionBudgetPerRound, which the floor exempts. gas_data
// replaces DEFAULT_GAS_DATA wholesale, so all required node fields are
// restated here.
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
	minProposeTimeout: '1',
	maxProposeTimeout: '340282366920938463463374607431768211455',
	minCommitTimeout: '1',
	maxCommitTimeout: '340282366920938463463374607431768211455',
	messageBudgetFloor: '2000',
};

// Ample balance so the outcome isolates the budget rule, not the balance.
local extra = {
	'balances': {
		'AQAAAAAAAAAAAAAAAAAAAAAAAAA=': 1000000,
	},
	gas_data: gasData,
};
local base = deploy_then.run('${jsonnetDir}/${fileBaseName}.py', 'do_emit');
{tags: util.features([['message', 'send'], ['balance'], ['fees', 'balance']], 'stable') + ['python'],
	entry: util.addPaths([base + extra + {next: [base.next[0] + extra]}])}
