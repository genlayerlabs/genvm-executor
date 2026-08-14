local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ['python', 'feature-storage-tree-map'], entry: util.addPaths([simple.run('${jsonnetDir}/issue_163.py')])}
