local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{
	tags: ["feature-message-send", "python"],
	entry: util.addPaths([
		simple_deploy.run('${jsonnetDir}/send_message.py') {
			bucket_totals: {execution_data_gas: 0, message_fee: messageBudget},
			message_fee_allocation: [{
				recipient: null,
				call_key: null,
				budget: 100,
				on: 'finalized',
				fee_params: {Internal: {
					leader_timeunits_allocation: 1,
					validator_timeunits_allocation: 1,
					execution_budget_per_round: 1,
					rotations: [0],
					max_price_gen_per_time_unit: 1,
					storage_fee_max_gas_price: 1,
					receipt_fee_max_gas_price: 1,
				}},
				children_budget: 60,
				subtree: [0, 255, 1, 2, 3],
			}],
		}
		for messageBudget in [60, 59]
	]),
}
