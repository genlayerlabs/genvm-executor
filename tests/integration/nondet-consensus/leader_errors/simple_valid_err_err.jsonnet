local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['nondet', 'consensus', 'leader', 'error']], 'stable') + ['python'],
	entry: util.addPaths([simple.run('${jsonnetDir}/simple.py', 'foo') {
	next: [super.next[0] {
		leader_nondet: [
			{
				"kind": "vm_error",
				"value": "exit_code 1"
			}
		],
	}],
}])}
