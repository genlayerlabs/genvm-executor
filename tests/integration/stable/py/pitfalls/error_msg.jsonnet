local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ["feature-user-error", "python"], entry: util.addPaths([simple.run('${jsonnetDir}/error_msg.py', '#error')])}
