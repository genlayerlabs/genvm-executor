local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ["feature-sandbox-det", "feature-storage", "python"], entry: util.addPaths([simple_deploy.run('${jsonnetDir}/${fileBaseName}.py')])}
