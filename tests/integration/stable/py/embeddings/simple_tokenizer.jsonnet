local simple = import 'templates/simple_deploy_then_write.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ["feature-prompt-tokenizer", "python"], entry: util.addPaths([simple.run('${jsonnetDir}/${fileBaseName}.py', 'main', [true])])}
