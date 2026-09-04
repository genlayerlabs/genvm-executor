local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';

// Use the deployed 15% split to prove the emitted budget covers both the
// consensus primary reserve and the carried direct-child budgets
local gasData = {
	storageUnitPrice: '1',
	receiptGasPerByte: '1',
	gasPerChangedSlot: '1',
	intrinsicGas: '0',
	bootloaderOverhead: '0',
	fixedProposeReceiptGas: '0',
	fixedMessageRevealGas: '0',
	lockedReceiptGasPrice: '1',
	overlaySplitBps: '1500',
	receiptWrapperBytes: '1024',
	genPerTimeUnit: '0',
	minProposeTimeout: '1',
	maxProposeTimeout: '340282366920938463463374607431768211455',
	minCommitTimeout: '1',
	maxCommitTimeout: '340282366920938463463374607431768211455',
	messageBudgetFloor: '0',
};

local params = {
	execution_budget_per_round: 1,
	rotations: [0],
	leader_timeunits_allocation: 1,
	validator_timeunits_allocation: 1,
	max_price_gen_per_time_unit: 2,
	storage_fee_max_gas_price: 1,
	receipt_fee_max_gas_price: 1,
};

local child(budget, children=[], recipient=null) = {
	budget: budget,
	recipient: recipient,
	call_key: null,
	on: 'finalized',
	fee_params: {Internal: params},
	children: children,
};

local alloc = child(100, [
	child(30, [child(13)]),
	child(30, [], 'AwAAAAAAAAAAAAAAAAAAAAAAAAA='),
]);

{
	tags: util.features([['message', 'send'], ['fees']], 'stable') + ['python'],
	entry: util.addPaths([
		simple_deploy.run('${jsonnetDir}/${fileBaseName}.py') {
			bucket_totals: {message_fee: 100},
			gas_data: gasData,
			message_fee_allocation: [alloc],
		},
	]),
}
