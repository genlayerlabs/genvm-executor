import json
import sys
from pathlib import Path

# executors/<line>.x/ — 3 parents up from docs/website/src/conf.py
_root = Path(__file__).resolve().parents[3]
_manifest = json.loads((_root / 'manifest.json').read_text())

# The docs are built from the manager tree, which owns the site-wide settings
_manager_root = _root
while not (_manager_root / '.genvm-monorepo-root').exists():
	_manager_root = _manager_root.parent
sys.path.insert(0, str(_manager_root / 'docs' / 'website' / 'common'))

import conf_common  # noqa: E402

release = _manifest['executor-version']
version = release

conf_common.apply(
	globals(),
	kind='executor',
	title='GenVM Executor',
	crumbs=[
		(f'Manager {conf_common.MONOREPO["version"]}', conf_common.MANAGER_PATH),
		(f'Executor {release}', conf_common.executor_path(release)),
	],
)

extensions = [
	'sphinx.ext.intersphinx',
	'sphinx.ext.todo',
	'sphinxcontrib.mermaid',
	'myst_parser',
]

exclude_patterns = [
	'_build',
	'Thumbs.db',
	'.DS_Store',
	# `.. include::`d into changelog.rst; excluding them from the doc set only
	# suppresses the "not in any toctree" orphan warnings (include still works).
	'python-sdk/changelog-notes/*',
]

master_doc = 'index'

mermaid_version = '11.6.0'
mermaid_output_format = 'raw'

todo_include_todos = True

# The manager, runner and executor sites reference each other, so the pipeline
# builds them twice; on the first pass an inventory may be missing
intersphinx_mapping = {
	'python': ('https://docs.python.org/3.12', None),
	'manager': (
		conf_common.relative('executor', conf_common.MANAGER_PATH),
		conf_common.inventory(conf_common.MANAGER_PATH),
	),
}

# `genlayer` objects the guides reference live on the runner page of the SDK
_runners = _manager_root / 'build' / 'doc' / 'runners.json'
if _runners.exists():
	_sdk = [
		r
		for r in json.loads(_runners.read_text())
		if r['id'] == 'py-lib-genlayer-std' and r['version'] == release and r['latest']
	]
	if len(_sdk) != 1:
		raise ValueError(
			f'executor {release}: expected 1 latest py-lib-genlayer-std, got {_sdk}'
		)
	_sdk_path = conf_common.runner_path(_sdk[0]['id'], _sdk[0]['hash32'])
	intersphinx_mapping['sdk'] = (
		conf_common.relative('executor', _sdk_path),
		conf_common.inventory(_sdk_path),
	)

# Unresolved :doc: references fall through to the other sites too
intersphinx_disabled_reftypes: list[str] = []
