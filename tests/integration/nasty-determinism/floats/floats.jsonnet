local msg = import 'templates/message.json';
local util = import 'templates/util.jsonnet';
local simple_deploy = import 'templates/simple_deploy.jsonnet';

{
	tags: util.features([['nasty-determinism', 'float']], 'stable') + ['fuzz', 'python'],
	entry: util.addPaths([
		simple_deploy.run('${jsonnetDir}/contract.py') {
			expected_semantics_components: ['return'],
			modes: 'lvs',
			stable_hash: true,
		},
	]),
}
