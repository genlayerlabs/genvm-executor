local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ["feature-nasty-determinism-float", "feature-nondet", "feature-prompt-embedding", "python", "slow"], entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py', 'main', [false])])}
