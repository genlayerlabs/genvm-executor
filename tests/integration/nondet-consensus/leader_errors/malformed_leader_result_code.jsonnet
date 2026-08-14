local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
// A result code byte outside {return, user_error, vm_error}. Used to trap
// ("invalid leader result code"), which would surface as an internal error and
// a timeout vote rather than a disagreement.
{tags: util.features([['nondet', 'consensus', 'leader', 'error']], 'stable') + ['python'],
	entry: util.addPaths([simple.run('${jsonnetDir}/simple.py', 'bar') {
	next: [super.next[0] {
		modes: 'vs',
		leader_nondet: [
			{
				"kind": "raw",
				"value": [7],
			},
		],
	}],
}])}
