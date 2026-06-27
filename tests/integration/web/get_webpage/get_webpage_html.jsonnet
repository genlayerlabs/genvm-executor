local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['web', 'render'], ['nondet']], 'unstable'),
	entry: util.addPaths([simple.run('${jsonnetDir}/get_webpage.py', 'main', ["html"]) {
	next: [super.next[0] {
		stable_hash: false,
	}],
}])}
