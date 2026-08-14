function(fn_name)

local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';

local raw(value) = { "kind": "raw", "value": value };
local ret(value) = { "kind": "return", "value": value };
local vm_err(code) = { "kind": "vm_error", "value": code };

// Every result code byte, including the two the wire format does not admit:
// 3 (`internal_error` — host-facing only, never proposable) and 7 (unknown).
local result_codes = [0, 1, 2, 3, 7];

// Calldata type tags are the low 3 bits: 0..6 are the defined types, so 7 is
// not a valid tag, and `null` encodes to the single byte 0x00.
local malformed_payloads = [
	[0, 7],        // return: payload is not decodable
	[0, 0, 0],     // return: valid `null` plus a trailing byte
	[1, 7],        // user_error: payload is not decodable
	[2, 255, 254], // vm_error: code is not valid UTF-8, so it needs raw bytes
];

// Well-formed UTF-8 that the `vm_error` trie must still reject.
local rejected_vm_error_codes = [
	'i_made_this_up', // not in the trie at all
	'exit_code +7',   // parameter is not spelled canonically
	'out_of',         // non-terminal trie node
];

local leader_nondets =
	[
		// Nothing proposed at all.
		[],
		// this one is OK
		[ret(11)],
		// this one is OK & disagreement
		[ret(1)],
		// Two well-formed results where the contract runs a single block: the
		// second block is never reached, so this pins down the surplus rule —
		// the run's result is replaced with `leader_fault nondet_output extra`.
		[ret(11), ret(11)],
	]
	+ [
		// A bare result code carrying no payload.
		[raw([code])]
		for code in result_codes
	]
	+ [
		[raw(value)]
		for value in malformed_payloads
	]
	+ [
		[vm_err(code)]
		for code in rejected_vm_error_codes
	];
local test_cases = [
	local ans = {
		"fn": fn_name,
		"leader_nondet": leader_nondet,
	};
	ans + { comment: std.manifestJsonMinified(ans) }
	for leader_nondet in leader_nondets
];
{
	tags: util.features([['nondet', 'consensus', 'leader', 'malicious'], ['exploit', 'leader-fault']], 'stable') + ['python'],
	entry: util.addPaths([
		simple_deploy.run('${jsonnetDir}/../malicious-leader.py') {
			modes: 'vs',
			leader_nondet: test.leader_nondet,
			// A deployment carries no method name: the calling convention's key
			// set is {"", "args", "kwargs"} and `""` must be absent when
			// `is_init` is set.
			"calldata": std.manifestJsonEx({
				"args": [test.fn, test.comment],
			}, "    "),
			slug: std.substr(std.native('gvm32')(std.manifestJson(test.comment)), 0, 6)
		}
		for test in test_cases
	]),
}
