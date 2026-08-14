local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ["feature-storage-nested", "feature-storage-tree-map", "python"], entry: util.addPaths([simple_deploy.run('${jsonnetDir}/tree_map_nested.py')])}
