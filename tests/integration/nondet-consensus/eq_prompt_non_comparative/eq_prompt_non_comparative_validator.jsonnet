local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['nondet', 'consensus', 'validator'], ['prompt', 'non-comparative']], 'unstable') + ['needs-llm', 'needs-web', 'python', 'slow'],
	entry: util.addPaths([simple_deploy.run('${jsonnetDir}/eq_prompt_non_comparative.py') {
	leader_nondet: [
		{
			"kind": "return",
			"value": "Rats make fantastic pets, being affectionate, intelligent, and playful. They form strong bonds with humans, learn tricks, and possess charming, adaptable personalities."
		}
	],
	modes: 'v',
	stable_hash: true,
}])}
