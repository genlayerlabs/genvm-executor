local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ["feature-schema-primitive", "python"], entry: util.addPaths([simple.run('${jsonnetDir}/prim_types.py', '#get-schema')])}
