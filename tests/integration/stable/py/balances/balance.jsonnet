local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ["feature-balance", "feature-message-external-view", "feature-message-send", "feature-message-view", "python"], entry: util.addPaths([simple_deploy.run('${jsonnetDir}/${fileBaseName}.py') {
	"balances": {
		"AQAAAAAAAAAAAAAAAAAAAAAAAAA=": 10,
	},
}])}
