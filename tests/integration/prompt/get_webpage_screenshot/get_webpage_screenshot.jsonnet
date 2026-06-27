local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['prompt', 'vision'], ['web', 'render'], ['nondet']], 'unstable'),
	entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py', 'main', ["text"]) {
	next: [super.next[0] {
		stable_hash: false,
	}],
}])}
