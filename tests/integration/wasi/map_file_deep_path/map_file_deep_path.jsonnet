local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['wasi', 'map-file']], 'stable') + ['python'],
	entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py') {stable_hash: false, permissions: 'wscn'}])}
