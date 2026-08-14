local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ["feature-storage-tree-map", "python"], entry: util.addPaths([simple.run('${jsonnetDir}/storage_tree_map.py')])}
