local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ["feature-message-send", "python"], entry: util.addPaths([simple_deploy.run('${jsonnetDir}/send_message.py')])}
