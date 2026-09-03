local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';

{
	tags: util.features([['nondet', 'consensus', 'leader'], ['fees']], 'stable') + ['python'],
	entry: util.addPaths([
		simple.run('${jsonnetDir}/${fileBaseName}.py', 'main') {
			next:
				local exact = super.next[0] {
					modes: 'lvs',
					// 64-byte frame + one 34-byte compact VMError output
					bucket_totals: {
						nondet_outputs: 98,
						// 71 message startup gas + 1024 wrapper + 34 output bytes
						execution_data_gas: 1129,
					},
				};
				[
					exact,
					exact {bucket_totals+: {nondet_outputs: 97}},
					exact {bucket_totals+: {execution_data_gas: 1128}},
				],
		},
	]),
}
