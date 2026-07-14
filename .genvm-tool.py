"""Executor-root genvm-tool project config.

Loaded by genvm-tool (``common.load_project``) for this executor submodule to
provide its build ``configure`` config. The umbrella test suite lives in the
manager's ``.genvm-tool.py``; commit hooks live in this repo's flake
(git-hooks.nix).
"""


def configure(line):
	"""Per-line build configuration for `genvm-tool configure`.

	``line`` is a ``genvm_tool_plugins.ninja.LineContext`` the configure command
	builds for this executor. This is a frozen legacy line: it ships a committed
	``executor/registry`` and the build copies those runner manifests verbatim
	(``frozen_registry``) rather than deriving them through the umbrella nix
	machinery, so the debug and nix builds agree on runner hashes.
	"""
	line.register_standard_codegen()
	line.register_standard_crates()
	line.frozen_registry()
	line.install_tree()
