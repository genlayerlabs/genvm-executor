local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{
	tags: ["feature-balance", "feature-message-send", "feature-sandbox-det", "octane", "python"],
	entry: util.addPaths([simple_deploy.run('${jsonnetDir}/${fileBaseName}.py') {
		//expected_semantics_components: [],
		modes: 'lvs',
		stable_hash: true,
		balances: {
			"AQAAAAAAAAAAAAAAAAAAAAAAAAA=": 100,
		},
	}]),
}
