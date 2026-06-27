local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: util.features([['sandbox', 'non-det'], ['nondet']], 'unstable'),
	entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py', 'main', ["gl.nondet.web.render('https://test-server.genlayer.com/static/genvm/hello.html', mode='text')"]) {
	next: [super.next[0] {
		stable_hash: false,
	}],
}])}
