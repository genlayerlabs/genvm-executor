local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ['python', 'feature-nondet-consensus-validator-rollback', 'feature-user-error'], entry: util.addPaths([simple.run('${jsonnetDir}/rollback.py', 'main') {
	next: [super.next[0] {
		leader_nondet: [
			{
				"kind": "rollback",
				"value": "rollback"
			}
		]
	}],
}])}
