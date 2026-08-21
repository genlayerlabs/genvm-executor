local simple_deploy_then_write = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
local r = simple_deploy_then_write.run('${jsonnetDir}/${fileBaseName}.py', 'bench');
{tags: util.features([['storage', 'tree-map']], 'stable') + ['bench', 'python', 'slow', 'needs-time'], entry: util.addPaths([
	util.updateField(r, 'next', function(next)
		util.updateArrayElement(next, 0, function(s) s + {benchmark: true, modes: 'l'})
	)
])}
