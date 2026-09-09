local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
// The envelope itself does not decode, so the fault is raised before the VM
// starts. It is a leader fault like any other, hence fatal
{tags: util.features([['nondet', 'consensus', 'leader', 'error']], 'stable') + ['python'],
	entry: util.addPaths([simple.run('${jsonnetDir}/simple.py', 'bar') {
	next: [super.next[0] {
		modes: 'vs',
		leader_public_data_raw: [255],
	}],
}])}
