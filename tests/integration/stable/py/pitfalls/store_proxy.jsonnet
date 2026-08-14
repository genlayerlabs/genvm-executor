local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ["feature-storage-dynamic-array", "feature-storage-nested", "python"], entry: util.addPaths([simple_deploy.run('${jsonnetDir}/store_proxy.py')])}
