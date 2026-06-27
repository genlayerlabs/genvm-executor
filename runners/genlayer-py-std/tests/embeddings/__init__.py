import json
import sys
from pathlib import Path

MONO_REPO_ROOT_FILE = '.genvm-executor-root'
script_dir = Path(__file__).parent.absolute()

root_dir = script_dir
while not root_dir.joinpath(MONO_REPO_ROOT_FILE).exists():
	root_dir = root_dir.parent
MONOREPO_CONF = json.loads(root_dir.joinpath(MONO_REPO_ROOT_FILE).read_text())

ppy_path = root_dir.joinpath(*MONOREPO_CONF['pure-py'])

sys.path.append(str(ppy_path))
