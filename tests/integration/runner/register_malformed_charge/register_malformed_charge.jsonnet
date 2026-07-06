local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
// ADR-012 §3: a malformed `RegisterRunner` is a deterministic invalid-contract
// error. The pre-parse charge is retained, but the failed parse produces no
// `runner load` record — so only the preceding valid registration is logged as
// `charged`, and nothing is logged for the malformed bytes.
{tags: util.features([['runner', 'register']], 'stable'),
	entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py') {
		stable_hash: false,
		permissions: 'rwscnu',
		runner_load_asserts: [
			{runner_prefix: 'custom:', match: {status: 'charged'}, count: 1},
			{runner_prefix: 'custom:', match: {status: 'cached'}, count: 0},
		],
	}])}
