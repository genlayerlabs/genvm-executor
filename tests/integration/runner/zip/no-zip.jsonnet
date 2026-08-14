local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([], 'stable') + ['python'],
	entry: util.addPaths([simple_deploy.run('${jsonnetDir}/contract.py')])}
