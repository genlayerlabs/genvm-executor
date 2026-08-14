import subprocess
import zipfile
from pathlib import Path

root = Path(__file__).parent

subprocess.run(
	[
		'wat2wasm',
		str(root / 'fd_write_iovec_overflow.wat'),
		'-o',
		str(root / 'fd_write_iovec_overflow.wasm'),
	],
	check=True,
)

with zipfile.ZipFile(root / 'contract.zip', 'w') as archive:
	for name in ['runner.json', 'fd_write_iovec_overflow.wasm']:
		archive.write(root / name, name)
