local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ["feature-schema", "python"], entry: util.addPaths([simple_deploy.run('${jsonnetDir}/pub_ctor.py')])}
