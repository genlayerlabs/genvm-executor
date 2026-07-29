local msg = import 'templates/message.json';
local util = import 'templates/util.jsonnet';

// A runner id the guest chooses at runtime, rather than one declared in a
// runner comment. All three shapes must come back as canonical VMErrors; an
// id that is merely uninstalled used to abort the transaction internally.
local step(method) = {
	vars: {},
	code: null,
	message: msg,
	calldata: std.manifestJsonEx({'': method, args: []}, '    '),
	stable_hash: false,
};

{
	tags: util.features([['runner']], 'stable'),
	entry: util.addPaths([{
		vars: {},
		code: '${jsonnetDir}/${fileBaseName}.py',
		message: msg + {is_init: true},
		calldata: '{}',
		stable_hash: false,
		next: [
			step('spawn_absent'),
			step('map_absent'),
			step('spawn_malformed'),
		],
	}])
}
