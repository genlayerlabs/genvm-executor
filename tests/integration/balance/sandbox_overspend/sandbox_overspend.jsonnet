local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{
	tags: util.features([['balance'], ['message', 'send'], ['sandbox', 'det']], 'stable') + ["octane", "python"],
	entry: util.addPaths([simple_deploy.run('${jsonnetDir}/${fileBaseName}.py') {
		//expected_semantics_components: [],
		modes: 'lvs',
		stable_hash: true,
		balances: {
			"AQAAAAAAAAAAAAAAAAAAAAAAAAA=": 100,
		},
	}]),
}
