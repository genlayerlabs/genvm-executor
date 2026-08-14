local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ['python'], entry: util.addPaths([simple.run('${jsonnetDir}/typing_is_ok.py')])}
