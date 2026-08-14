local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ["feature-message-external", "python"], entry: util.addPaths([simple.run('${jsonnetDir}/methods.py', 'pub')])}
