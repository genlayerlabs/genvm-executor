local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
// First load of a runner charges exactly RUNNER_LOAD_COST + size, once.
// A bare raw-wasm contract's runner tree is a single node, so exactly one
// `charged` record must appear, sized to the raw wasm length, none `cached`.
{tags: util.features([['runner', 'load']], 'stable') + ['wasm'],
	entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.wat') {
		runner_load_asserts: [
			{match: {}, count: 1},
			{match: {status: 'charged'}, count: 1, size_is_code_len: true},
			{match: {runner_load_cost: 4096}, count: 1},
			{match: {status: 'cached'}, count: 0},
		],
	}])}
