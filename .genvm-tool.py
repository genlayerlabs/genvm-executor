"""Executor-root genvm-tool project config.

Loaded by genvm-tool (``common.load_project``) for this executor submodule to
provide its build ``configure``/``integration`` config. The umbrella test suite
lives in the manager's ``.genvm-tool.py``; commit hooks live in this repo's
flake (git-hooks.nix).
"""


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
	# Pending public-abi constants (ADR-012): constants that logically belong to
	# the public ABI but are parked here so they do not regenerate the runner
	# `public_abi.py` (which would change frozen runner hashes). Generated into the
	# `common` crate only — never the runner tree.
	line.codegen(
		line.exec_root / 'executor/crates/common/src/public_abi_pending.rs',
		'rust',
		line.exec_root / 'executor/codegen/data/public-abi-pending.json',
	)
	line.register_standard_crates()
	line.nix_manifests()
	line.install_tree()
