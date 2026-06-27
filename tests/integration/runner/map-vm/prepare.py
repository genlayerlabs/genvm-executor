import zipfile
from pathlib import Path

test_dir = Path(__file__).parent

with zipfile.ZipFile(test_dir.joinpath('contract.zip'), 'w') as f:
	for name in ['contract.py', 'runner.json']:
		f.write(test_dir.joinpath(name), name)
