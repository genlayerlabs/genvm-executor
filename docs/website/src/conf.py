import datetime
import json
import os
from pathlib import Path

# executors/<line>.x/ — three parents up from docs/website/src/conf.py
_root = Path(__file__).resolve().parents[3]
_manifest = json.loads((_root / 'manifest.json').read_text())

project = 'GenVM Executor'
author = 'GenLayer Labs'

copyright_year = os.environ.get('COPYRIGHT_YEAR', str(datetime.date.today().year))
copyright = f'{copyright_year}, GenLayer Labs'

# This sub-site's version is always the line's own executor-version. Do NOT
# read DOCS_VERSION here: the deploy sets it to the *manager* version, which
# would otherwise leak in and make every line's sub-site claim the same version.
release = _manifest['executor-version']
version = release

extensions = [
	'myst_parser',
	'sphinxcontrib.mermaid',
]

exclude_patterns = ['_build', 'Thumbs.db', '.DS_Store']
language = 'en'

mermaid_version = '11.6.0'
mermaid_output_format = 'svg'
mermaid_params = ['--theme', 'dark', '--backgroundColor', 'transparent']

html_theme = 'pydata_sphinx_theme'

master_doc = 'index'
