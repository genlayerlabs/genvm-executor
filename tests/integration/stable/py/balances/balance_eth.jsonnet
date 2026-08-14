local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ["feature-balance", "feature-message-eth", "feature-message-external-view", "feature-message-send-eth", "feature-message-view", "python"], entry: util.addPaths([simple_deploy.run('${jsonnetDir}/balance_eth.py') {
	"balances": {
		"AQAAAAAAAAAAAAAAAAAAAAAAAAA=": 10,
	},
}])}
