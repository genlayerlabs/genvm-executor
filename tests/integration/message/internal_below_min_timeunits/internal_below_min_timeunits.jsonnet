local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';

// Pin a per-phase timeunit floor above the emitted message's child timeunits.
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
	// leader 5 / validator 10 (below) are rejected at emission.
	minTimeUnitsPerPhase: '30',
};

// A single wildcard internal allocation that matches the emitted message, funded
// with an ample (but never-reached) budget so the rejection isolates the timeunit
// floor from any fee/budget effect. leader 5 and validator 10 are both < 30.
local alloc = {
	budget: 9000000000000000,
	recipient: null,
	call_key: null,
	on: 'finalized',
	fee_params: {
		Internal: {
			execution_budget_per_round: 1024,
			rotations: [4, 4, 4, 4, 4],
			leader_timeunits_allocation: 5,
			validator_timeunits_allocation: 10,
			max_price_gen_per_time_unit: 9000000000000000,
			storage_fee_max_gas_price: 20,
			receipt_fee_max_gas_price: 20,
		},
	},
	children: [],
};

{tags: util.features([['message'], ['fees']], 'stable'),
	entry: util.addPaths([
		simple_deploy.run('${jsonnetDir}/${fileBaseName}.py') {
			gas_data: gasData,
			message_fee_allocation: [alloc],
		},
	])}
