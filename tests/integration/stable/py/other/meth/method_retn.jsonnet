local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ["feature-message-external", "feature-schema-complex", "python"], entry: util.addPaths([simple.run('${jsonnetDir}/methods.py', 'retn')])}
