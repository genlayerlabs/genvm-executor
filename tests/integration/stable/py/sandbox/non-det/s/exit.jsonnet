local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ["feature-sandbox-non-det", "python"], entry: util.addPaths([simple.run('${jsonnetDir}/../code.py', 'main', ["exit(1)"])])}
