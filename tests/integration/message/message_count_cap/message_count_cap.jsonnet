local simpleDeploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';

local base = simpleDeploy.run('${jsonnetDir}/../send_message/send_message.py');
{tags: util.features([['message', 'send'], ['fees']], 'stable') + ['python'],
	entry: util.addPaths([
		base {bucket_totals: {submitted_messages_count: 1}},
		base {bucket_totals: {submitted_messages_count: 0}},
		// 64-byte array frame + one 1888-byte conservatively encoded message
		base {bucket_totals: {submitted_messages: 1952}},
		base {bucket_totals: {submitted_messages: 1951}},
		// 1095 startup + 12 storage + 1953 message receipt gas
		base {bucket_totals: {execution_data_gas: 3060}},
		base {bucket_totals: {execution_data_gas: 3059}},
	]),
}
