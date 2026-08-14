local simple = import 'templates/simple.jsonnet';
local util = import 'templates/util.jsonnet';
{tags: ["feature-message-external", "python"], entry: util.addPaths([simple.run('${jsonnetDir}/methods.py') {
	"calldata": |||
		{
			"": "pub",
			"args": []
		}
	|||,
	"message": super.message + {
		"is_init": true,
	}
}])}
