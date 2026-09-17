local simple = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{
	tags: util.features([['nondet', 'consensus', 'validator']], 'stable') + ['python'],
	entry: [step + (if step.mode == 'v' then {
		expected_semantics_components: ['stdout', 'return'],
	} else {}) for step in util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py') {
		allow_two_workers: false,
	}])],
}
