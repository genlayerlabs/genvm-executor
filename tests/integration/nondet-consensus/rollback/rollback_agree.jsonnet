local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['nondet', 'consensus', 'validator', 'rollback']], 'stable') + ['python'],
	entry: util.addPaths([simple.run('${jsonnetDir}/rollback.py', 'main') {
	next: [super.next[0] {
		leader_nondet: [
			{
				"kind": "user_error",
				"value": "rollback"
			}
		]
	}],
}])}
