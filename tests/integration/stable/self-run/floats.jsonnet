local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ['python', 'feature-nasty-determinism-float'], entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py')])}
