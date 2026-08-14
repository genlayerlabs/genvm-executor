local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ['python'], entry: util.addPaths([simple_deploy.run('${jsonnetDir}/contract.py')])}
