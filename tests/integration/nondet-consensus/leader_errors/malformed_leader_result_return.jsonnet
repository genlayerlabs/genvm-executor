local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
// A `return` whose payload is not decodable calldata. This is the hole being
// closed — the payload used to be passed through undecoded, straight into the
// execution hash.
{tags: util.features([['nondet', 'consensus', 'leader'], ['nondet']], 'stable'),
	entry: util.addPaths([simple.run('${jsonnetDir}/simple.py', 'bar') {
	next: [super.next[0] {
		modes: 'vs',
		leader_nondet: [
			{
				"kind": "raw",
				"value": [0, 255, 255, 255],
			},
		],
	}],
}])}
