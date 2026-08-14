local simple = import 'templates/simple.jsonnet';
local s = simple.run('${jsonnetDir}/code.py');
local util = import 'templates/util.jsonnet';
{
	tags: util.features([['storage', 'lock']], 'stable') + ['python'],
	entry: util.addPaths([util.chain([
		// step 0: deploy, grow upgraders to 33 (> limit of 32)
		s {
			calldata: |||
				{
					"args": [33],
				}
			|||,
			message: super.message + { is_init: true },
		},
		// step 1: a write call -- the supervisor reads the over-limit set and the
		// run must return `out_of upgraders` as a receipt (nop body never runs)
		s {
			code: null,
			calldata: |||
				{
					"": "nop",
				}
			|||,
		},
	])]),
}
