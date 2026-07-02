local msg = import 'templates/message.json';
local util = import 'templates/util.jsonnet';
{
	tags: util.features([['runner', 'slot']], 'stable'),
	entry: util.addPaths([{
		vars: {},
		code: '${jsonnetDir}/${fileBaseName}.py',
		message: msg + {is_init: true},
		calldata: '{}',
		stable_hash: false,
		next: [{
			vars: {},
			code: null,
			message: msg,
			calldata: std.manifestJsonEx({'': 'foo', args: []}, '    '),
			stable_hash: false,
		}],
	}])
}
