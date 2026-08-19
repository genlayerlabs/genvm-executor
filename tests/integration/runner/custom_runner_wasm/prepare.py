import json
import os
import subprocess
import zipfile
from pathlib import Path

root = Path(__file__).parent
repo_root = next(p for p in root.parents if (p / '.genvm-executor-root').exists())

_build_info_env = os.environ.get('GENVM_BUILD_INFO')
_build_info_path = (
	Path(_build_info_env) if _build_info_env else repo_root.joinpath('build', 'info.json')
)
build_info = json.loads(_build_info_path.read_text())
try:
	_mount = str(repo_root.relative_to(Path(build_info['build_dir']).parent))
except (KeyError, ValueError):
	_mount = ''
target_dir = Path(
	build_info.get('rust_target_dirs', {}).get(_mount, build_info['rust_target_dir'])
)

subprocess.run(
	[
		'cargo',
		'build',
		'--example',
		'fibonacci',
		'--target',
		'wasm32-wasip1',
		'--release',
		'--target-dir',
		str(target_dir),
	],
	cwd=repo_root / 'executor' / 'crates' / 'sdk-rs',
	check=True,
)

src = target_dir / 'wasm32-wasip1' / 'release' / 'examples' / 'fibonacci.wasm'
wasm = root / 'fibonacci.wasm'
wat = root / 'fibonacci.wat'

# Round-trip through wabt to normalize to MVP format
subprocess.run(['wasm2wat', str(src), '-o', str(wat)], check=True)
subprocess.run(['wat2wasm', str(wat), '-o', str(wasm)], check=True)
wat.unlink()

with zipfile.ZipFile(root / 'contract.zip', 'w') as f:
	f.write(root / '__init__.py', 'contract/__init__.py')
	f.write(wasm, 'fibonacci.wasm')
	f.write(root / 'runner.json', 'runner.json')
