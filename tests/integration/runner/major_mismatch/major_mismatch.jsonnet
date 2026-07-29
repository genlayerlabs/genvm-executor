local two = import 'templates/two.jsonnet';
local msg = import 'templates/message.json';
local util = import 'templates/util.jsonnet';

// every entry point that reads a contract's (mismatched) major must reject it:
local entries = [
	// 0: top-level call to a major-mismatched contract
	{
		vars: {},
		code: '${jsonnetDir}/target.py',
		message: msg + {is_init: true},
		calldata: '{}',
		next: [{
			vars: {},
			code: null,
			message: msg,
			calldata: std.manifestJsonEx({'': 'foo', args: []}, '    '),
			// The host claims a major this executor serves, because the subject
			// here is the executor's own check rather than the manager's
			// routing. Entry 3 covers the other half, where the host reports
			// the contract's real (unservable) major.
			major: 0,
		}],
	},
	// 1: CallContract into a major-mismatched contract
	two.run('${jsonnetDir}/caller.py', '${jsonnetDir}/target.py',
		|||
			{
				"": "call",
				"args": [Address(toAddr)]
			}
		|||
	),
	// 2: map_file from a major-mismatched contract
	two.run('${jsonnetDir}/caller.py', '${jsonnetDir}/target.py',
		|||
			{
				"": "map",
				"args": [Address(toAddr)]
			}
		|||
	),
	// 3: the host reports the contract's real major, which no installed line
	// provides. The manager has nothing to route to, and a major written by a
	// contract into its own root slot must not be able to abort the run: it
	// falls back to the newest line, whose own check answers with the same
	// canonical error as entry 0.
	{
		vars: {},
		code: '${jsonnetDir}/target.py',
		message: msg + {is_init: true},
		calldata: '{}',
		next: [{
			vars: {},
			code: null,
			message: msg,
			calldata: std.manifestJsonEx({'': 'foo', args: []}, '    '),
		}],
	},
];

{tags: util.features([['runner', 'malformed'], ['message']], 'stable'),
	entry: util.addPaths(util.mapGraph(function(e) e + {stable_hash: false}, entries))}
