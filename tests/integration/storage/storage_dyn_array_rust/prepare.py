import json
import os
import subprocess
from pathlib import Path

root = Path(__file__).parent
# Resolve the executor checkout root by marker, independent of how deep
# this test sits under tests/ (so moving cases never breaks this).
repo_root = next(p for p in root.parents if (p / '.genvm-executor-root').exists())
# absolute build-info path via GENVM_BUILD_INFO. Fall back to the old relative
# location for standalone use.
_build_info_env = os.environ.get('GENVM_BUILD_INFO')
_build_info_path = (
	Path(_build_info_env) if _build_info_env else repo_root.joinpath('build', 'info.json')
)
build_info = json.loads(_build_info_path.read_text())
target_dir = Path(build_info['rust_target_dir'])

subprocess.run(
	[
		'cargo',
		'build',
		'--example',
		'storage_dyn_array',
		'--target',
		'wasm32-wasip1',
		'--release',
		'--target-dir',
		str(target_dir),
	],
	cwd=repo_root / 'executor' / 'crates' / 'sdk-rs',
	check=True,
)

src = target_dir / 'wasm32-wasip1' / 'release' / 'examples' / 'storage_dyn_array.wasm'
dst = root / 'storage_dyn_array.wasm'
wat = root / 'storage_dyn_array.wat'

# Round-trip through wabt to normalize to MVP format
subprocess.run(['wasm2wat', str(src), '-o', str(wat)], check=True)
subprocess.run(['wat2wasm', str(wat), '-o', str(dst)], check=True)
wat.unlink()
