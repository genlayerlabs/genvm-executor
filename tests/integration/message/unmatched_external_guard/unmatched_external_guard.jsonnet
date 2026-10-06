local simpleDeploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';

local guard = {
	budget: 0,
	recipient: null,
	call_key: null,
	on: 'finalized',
	fee_params: {
		External: {
			gas_limit: 1,
			max_gas_price: '115792089237316195423570985008687907853269984665640564039457584007913129639935',
		},
	},
	children_budget: 0,
	subtree: [],
};

local pinned = {
	budget: 1000,
	recipient: 'ERERERERERERERERERERERERERE=',
	call_key: null,
	on: 'finalized',
	fee_params: {
		External: {
			gas_limit: 100,
			max_gas_price: 1,
		},
	},
	children_budget: 0,
	subtree: [],
};

local run = simpleDeploy.run('${jsonnetDir}/${fileBaseName}.py');
{
	tags: util.features([['message', 'send', 'eth'], ['fees']], 'stable') + ['python'],
	entry: util.addPaths([
		run + {
			slug: 'pinned',
			message_fee_allocation: [pinned, guard],
		},
		run + {
			slug: 'closed',
			message_fee_allocation: [guard],
		},
		run + {
			slug: 'no_allocations',
			message_fee_allocation: [],
		},
	]),
}
