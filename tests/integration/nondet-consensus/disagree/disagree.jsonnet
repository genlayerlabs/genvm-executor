local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['nondet', 'consensus', 'validator'], ['web', 'render']], 'unstable') + ['needs-web', 'python'],
	entry: util.addPaths([simple.run('${jsonnetDir}/../../web/get_webpage/get_webpage.py', 'main', ["text"]) {
	next: [super.next[0] {
		modes: 'v',
		leader_nondet: [
			{
				"kind": "return",
				"value": "Hello world~"
			}
		],
		stable_hash: true,
	}],
}])}
