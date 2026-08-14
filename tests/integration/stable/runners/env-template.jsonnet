local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['runner'], ['wasi', 'environment']], 'stable') + ['python'], entry: util.addPaths([simple.run('${jsonnetDir}/env-template.py')])}
