local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ['python', 'feature-nondet-consensus-leader-error'], entry: util.addPaths([simple.run('${jsonnetDir}/simple.py', 'foo')])}
