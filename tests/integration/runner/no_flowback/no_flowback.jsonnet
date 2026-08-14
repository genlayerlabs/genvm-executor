local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
// A sandbox-scoped `RegisterRunner` does not flow back to the
// parent. The child charges the custom exactly once (`charged`); the parent's
// later attempt to resolve that id fails deterministically (no parent charge
// record for it) and the run ends as an invalid-contract VM error.
{tags: util.features([['runner', 'custom'], ['runner', 'load'], ['runner', 'register'], ['sandbox', 'det']], 'stable') + ['python'],
	entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py') {
		stable_hash: false,
		permissions: 'wscn',
		runner_load_asserts: [
			{runner_prefix: 'custom:', match: {status: 'charged'}, count: 1},
		],
	}])}
