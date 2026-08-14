local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ['python', 'needs-web', 'feature-nondet', 'feature-web-render', 'feature-permission-module'], entry: util.addPaths([simple.run('${jsonnetDir}/../py/other/meth/methods.py', 'det_viol')])}
