local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ["feature-message-payable", "feature-schema-primitive", "python"], entry: util.addPaths([simple.run('${jsonnetDir}/trivial.py', '#get-schema')])}
