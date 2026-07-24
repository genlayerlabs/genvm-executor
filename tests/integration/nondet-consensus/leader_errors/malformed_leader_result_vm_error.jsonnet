local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
// A `vm_error` whose code is not in the `vm_error` trie ("i_made_this_up"):
// a leader may not invent error codes.
{tags: util.features([['nondet', 'consensus', 'leader'], ['nondet']], 'stable'),
	entry: util.addPaths([simple.run('${jsonnetDir}/simple.py', 'bar') {
	next: [super.next[0] {
		modes: 'vs',
		leader_nondet: [
			{
				"kind": "raw",
				"value": [2, 105, 95, 109, 97, 100, 101, 95, 116, 104, 105, 115, 95, 117, 112],
			},
		],
	}],
}])}
