"""Executor-root genvm-tool project config.

Loaded by genvm-tool (``common.load_project``) for this executor submodule; the
hook engine asks it for ``hooks(ctx)`` (was ``support/nix/precommit/hooks.toml``).
The umbrella test suite lives in the manager's ``.genvm-tool.py``.
"""

import genvm_tool


def hooks(ctx: 'genvm_tool.common.Context'):
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
			'id': 'toml-format',
			'nix': 'taplo',
			'entry': 'taplo',
			'args': ['format', '--check'],
			'fix_args': ['format'],
			'types_or': ['toml'],
			'pass_filenames': True,
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
			# Refuse commits that leave dev-mode on or any hash as "test".
			# On failure prints docs/contributing/howto/committing/runners.md.
			'id': 'no-commit-dev-mode',
			'local': True,
			'entry': ctx.python_command[0],
			'args': [
				*ctx.python_command[1:],
				'runners/support/versions/check-dev-mode.py',
			],
			'files': r'^runners/support/versions/(dev-mode|current)\.nix$',
			'pass_filenames': False,
		},
		# --- commit-msg ----------------------------------------------------
		{
			'id': 'check-commit-message',
			'local': True,
			'entry': ctx.python_command[0],
			'args': [
				*ctx.python_command[1:],
				str(ctx.root / 'support/scripts/check-commit-message.py'),
				'--message-file',
			],
			'stages': ['commit-msg'],
			'pass_filenames': False,
		},
	]


def integration():
	"""Per-executor integration test configuration.

	Returned dict is merged with the harness defaults; keys:
		``ignore-hash`` — skip hash comparison for all tests in this executor line.
	"""
	return {
		'ignore-hash': True,
	}


def configure(line):
	"""Per-line build configuration for `genvm-tool configure`.

	``line`` is a ``genvm_tool_plugins.ninja.LineContext`` the configure command
	builds for this executor. This is a live line: its runner ``latest``/``all``
	manifests are derived through the umbrella nix machinery (``nix_manifests``),
	which imports every active line's runners.
	"""
	line.register_standard_codegen()
	line.register_standard_crates()
	line.nix_manifests()
	line.install_tree()
