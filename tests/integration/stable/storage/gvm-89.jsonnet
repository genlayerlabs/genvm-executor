local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ["feature-storage-dynamic-array", "feature-storage-nested", "python"], entry: util.addPaths([simple.run('${jsonnetDir}/gvm-89.py', 'main')])}
