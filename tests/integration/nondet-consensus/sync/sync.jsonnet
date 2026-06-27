local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['nondet', 'consensus', 'validator'], ['nondet']], 'stable'),
	entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py', 'main') {
	next: [super.next[0] {
		modes: 's',
		leader_nondet: [
			{
				"kind": "return",
				"value": "123"
			}
		]
	}],
}])}
