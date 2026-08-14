local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ["feature-sandbox-det", "feature-user-error", "python"], entry: util.addPaths([simple.run('${jsonnetDir}/../code.py', 'main', ["gl.advanced.user_error_immediate('RB')"])])}
