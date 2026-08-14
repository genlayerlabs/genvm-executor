local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ["feature-nondet", "feature-sandbox-non-det", "feature-web-render", "needs-web", "python"], entry: util.addPaths([simple.run('${jsonnetDir}/../code.py', 'main', ["gl.nondet.web.render('https://test-server.genlayer.com/static/genvm/hello.html', mode='text')"])])}
