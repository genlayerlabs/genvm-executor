local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';

{
	tags: util.features([['nondet', 'consensus', 'leader'], ['fees']], 'stable') + ['python'],
	entry: util.addPaths([
		simple.run('${jsonnetDir}/${fileBaseName}.py', 'main') {
			next: [
				super.next[0] {
					modes: 'lvs',
					// VMError byte + "out_of receipt nondet_output"
					bucket_totals: [1000000000, 1000000000, 29, 1000000000],
				},
			],
		},
	]),
}
