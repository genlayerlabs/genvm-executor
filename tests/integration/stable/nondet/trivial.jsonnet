local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ['python', 'feature-nondet-consensus'], entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py', 'init')])}
