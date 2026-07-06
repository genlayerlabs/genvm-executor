local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
// Regression for action-application stack use: a sandbox VM resolves a custom
// runner whose init action depends on a long chain of custom runners.
{tags: util.features([['runner', 'custom'], ['runner', 'dependency'], ['sandbox']], 'stable'),
	entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py') {stable_hash: false, permissions: 'rwscnu'}])}
