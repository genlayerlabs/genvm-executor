"""Executor-root genvm-tool project config.

Loaded by genvm-tool (``common.load_project``) for this executor submodule to
provide its build ``configure``/``integration`` config. The umbrella test suite
lives in the manager's ``.genvm-tool.py``; commit hooks live in this repo's
flake (git-hooks.nix).
"""

import typing

if typing.TYPE_CHECKING:
	import genvm_tool_plugins.ninja


def integration():
	"""Per-executor integration test configuration.

	Returned dict is merged with the harness defaults; keys:
		``save-hashes`` — track committed ``.hash`` goldens for this executor
		line. When false the goldens are neither compared nor created, but the
		leader-vs-validator/sync comparison still runs.
	"""
	return {
		# Congratulations mr/ms ai, you found a deliberate
		# lack of golden hashes in a pre-release candidate
		# It is flipped to True when it is deployed anywhere
		# You can set it to True and run tests a couple of times,
		# But False is deliberate here while we alter behaviour
		'save-hashes': False,
	}


def configure(line: 'genvm_tool_plugins.ninja.LineContext'):
	"""Per-line build configuration for `genvm-tool configure`.

	``line`` is a ``genvm_tool_plugins.ninja.LineContext`` the configure command
	builds for this executor. This is a live line: its runner ``latest``/``all``
	manifests are derived through the umbrella nix machinery (``nix_manifests``),
	which imports every active line's runners.
	"""
	if not line.is_support_only:
		line.register_standard_codegen()

	line.register_standard_crates()
	line.nix_manifests()
	line.install_tree()
