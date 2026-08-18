import subprocess
import zipfile
from pathlib import Path

root = Path(__file__).parent

subprocess.run(
	[
		'wat2wasm',
		str(root / 'softfloat_trunc_trap.wat'),
		'-o',
		str(root / 'softfloat_trunc_trap.wasm'),
	],
	check=True,
)

with zipfile.ZipFile(root / 'contract.zip', 'w') as archive:
	for name in ['runner.json', 'softfloat_trunc_trap.wasm']:
		archive.write(root / name, name)
