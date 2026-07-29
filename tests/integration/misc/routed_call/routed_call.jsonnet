// The host routes the callee to another executor instead of running it
// in-process, so the call leaves this process and comes back as a nested run.
//
// The callee pins its runner by hash instead of using `:test`, because the
// nested executor the manager spawns runs with debug mode disabled, where
// `:test` does not resolve.
local simple = import 'templates/two.jsonnet';
local util = import 'templates/util.jsonnet';
local base = simple.run('${jsonnetDir}/routed_call_from.py', '${jsonnetDir}/routed_call_to.py', |||
	{
		"": "main",
		"args": [Address(toAddr)]
	}
|||
);
{tags: util.features([['message', 'external', 'view']], 'stable'),
	entry: util.addPaths([base {
		next: [super.next[0] {
			next: [super.next[0] {
				executor_routes: {
					"AwAAAAAAAAAAAAAAAAAAAAAAAAA=": 3,
				},
			}],
		}],
	}])}
