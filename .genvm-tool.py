"""Executor-root genvm-tool project config.

Loaded by genvm-tool (``common.load_project``) for this executor submodule; the
hook engine asks it for ``hooks(ctx)`` (was ``support/nix/precommit/hooks.toml``).
The umbrella test suite lives in the manager's ``.genvm-tool.py``.
"""


def hooks(ctx):
	"""Executor commit-hook definitions (was support/nix/precommit/hooks.toml).

	Tools resolve from the sibling flake's buildEnv (``nix = "<flake output>"``);
	``builtin`` hooks run logic baked into genvm-tool. Formatters run in --check
	mode so a hook's exit code alone decides pass/fail. ``files``/``exclude`` are
	repo-relative (i.e. relative to executors/v0.3.x).
	"""
	return [
		# --- generic checks (mirror the manager's) -------------------------
		{
			'id': 'trailing-whitespace',
			'nix': 'pre-commit-hooks',
			'entry': 'trailing-whitespace-fixer',
			'types_or': ['text'],
			'exclude': r'^\.git-third-party|/fuzz/',
		},
		{
			'id': 'end-of-file-fixer',
			'nix': 'pre-commit-hooks',
			'entry': 'end-of-file-fixer',
			'types_or': ['text'],
			'exclude': r'^\.git-third-party|/fuzz/',
		},
		{
			'id': 'check-added-large-files',
			'nix': 'pre-commit-hooks',
			'entry': 'check-added-large-files',
		},
		{
			'id': 'check-json',
			'nix': 'pre-commit-hooks',
			'entry': 'check-json',
			'types_or': ['json'],
			'exclude': r'^\.git-third-party',
		},
		{
			'id': 'check-yaml',
			'nix': 'pre-commit-hooks',
			'entry': 'check-yaml',
			'types_or': ['yaml'],
		},
		{
			'id': 'check-toml',
			'nix': 'pre-commit-hooks',
			'entry': 'check-toml',
			'types_or': ['toml'],
		},
		{
			'id': 'check-merge-conflict',
			'nix': 'pre-commit-hooks',
			'entry': 'check-merge-conflict',
			'types_or': ['text'],
		},
		# --- executor-owned languages: python, c/c++, rust -----------------
		{
			'id': 'ruff-format',
			'nix': 'ruff',
			'entry': 'ruff',
			'args': ['format', '--check'],
			'fix_args': ['format'],
			'types_or': ['python'],
		},
		{
			'id': 'clang-format',
			'nix': 'clang-tools',
			'entry': 'clang-format',
			'args': ['--dry-run', '--Werror'],
			'fix_args': ['-i'],
			'types_or': ['c', 'c++'],
			'exclude': r'runners/softfloat/berkeley-softfloat-3|runners/py-libs',
		},
		{
			'id': 'editorconfig-checker',
			'nix': 'editorconfig-checker',
			'entry': 'editorconfig-checker',
			# text-only: keep binaries (e.g. runners/models/*.onnx) out of it.
			'types_or': ['text'],
			'exclude': r'\.git-third-party|runners/py-libs|runners/genlayer-py-std/src-emb/onnx|/fuzz/',
		},
		{
			'id': 'cargo-fmt',
			'nix': 'cargo',
			'builtin': 'cargo-fmt',
			'files': r'\.rs$',
			'pass_filenames': False,
		},
		{
			'id': 'nixfmt',
			'nix': 'nixfmt',
			'entry': 'nixfmt',
			'args': ['--check'],
			'fix_args': [],
			'files': r'\.nix$',
		},
		{
			'id': 'markdown-local-links',
			'builtin': 'md-local-links',
			'types_or': ['markdown'],
		},
		# --- local guard ---------------------------------------------------
		{
			# Refuse commits that leave the dev-mode runner toggle on (false).
			'id': 'no-commit-test',
			'local': True,
			'entry': 'grep',
			'args': ['-P', 'false', 'runners/support/versions/dev-mode.nix'],
			'files': r'^runners/support/versions/dev-mode\.nix$',
			'pass_filenames': False,
		},
	]
