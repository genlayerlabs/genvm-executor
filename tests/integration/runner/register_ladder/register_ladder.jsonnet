local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
// A registered custom runner is charged once (`charged`); a same-VM
// re-register of identical code is free (`cached`). Isolated from the noisy
// python runner tree via `runner_prefix: 'custom:'`.
{tags: util.features([['runner', 'register']], 'stable'),
	entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py') {
		stable_hash: false,
		permissions: 'wscn',
		runner_load_asserts: [
			{runner_prefix: 'custom:', match: {status: 'charged'}, count: 1},
			{runner_prefix: 'custom:', match: {status: 'cached'}, count: 1},
			{runner_prefix: 'custom:', match: {runner_load_cost: 4096}, min: 1},
		],
	}])}
