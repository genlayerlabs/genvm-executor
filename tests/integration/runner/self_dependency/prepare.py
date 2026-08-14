import subprocess
import zipfile
from pathlib import Path

root = Path(__file__).parent

subprocess.run(
	[
		'wat2wasm',
		str(root / 'self_dependency.wat'),
		'-o',
		str(root / 'self_dependency.wasm'),
	],
	check=True,
)

with zipfile.ZipFile(root / 'contract.zip', 'w') as archive:
	for name in ['runner.json', 'self_dependency.wasm']:
		archive.write(root / name, name)
