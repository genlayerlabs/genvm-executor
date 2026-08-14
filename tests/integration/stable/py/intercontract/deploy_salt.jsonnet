local simple_deploy = import 'templates/simple_deploy.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ["feature-message-deploy-salt", "python"], entry: util.addPaths([simple_deploy.run('${jsonnetDir}/deploy_salt.py')])}
