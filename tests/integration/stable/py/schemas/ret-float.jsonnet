local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ["feature-schema-float", "python"], entry: util.addPaths([simple.run('${jsonnetDir}/ret-float.py', '#get-schema')])}
