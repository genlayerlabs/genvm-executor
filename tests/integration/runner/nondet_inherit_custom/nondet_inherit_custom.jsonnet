local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
// A nondet block inherits the parent's custom runners by
// default, so a det-registered runner is usable inside `run_nondet`. Both the
// parent and the nondet child charge it once each (`charged` >= 2 across the
// run); each VM loads it at most once (no in-VM double charge).
{tags: util.features([['runner', 'custom'], ['runner', 'load'], ['runner', 'permission'], ['permission', 'runner'], ['sandbox', 'non-det'], ['nondet']], 'stable') + ['python'],
	entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py') {
		stable_hash: false,
		permissions: 'wscn',
		runner_load_asserts: [
			{runner_prefix: 'custom:', match: {status: 'charged'}, min: 2},
		],
	}])}
