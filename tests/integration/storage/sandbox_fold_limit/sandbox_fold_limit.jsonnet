local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';

local storageOnlyGasData = {
	storageUnitPrice: '0',
	receiptGasPerByte: '0',
	gasPerChangedSlot: '0',
	intrinsicGas: '0',
	bootloaderOverhead: '0',
	fixedProposeReceiptGas: '0',
	fixedMessageRevealGas: '0',
	genPerTimeUnit: '0',
};

local base = simple.run('${jsonnetDir}/${fileBaseName}.py');
{tags: util.features([['storage'], ['sandbox']], 'stable') + ['python', 'slow'],
	entry: util.addPaths([base + {
		// Validator and sync modes would make their multi-GiB allocations concurrently.
		modes: 'l',
		bucket_totals: [1000000000, 1000000000, 1000000000, 1000000000],
		gas_data: storageOnlyGasData,
	}])}
