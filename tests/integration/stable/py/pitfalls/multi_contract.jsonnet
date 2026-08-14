local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ["feature-user-error", "python"], entry: util.addPaths([simple_deploy.run('${jsonnetDir}/multi_contract.py')])}
