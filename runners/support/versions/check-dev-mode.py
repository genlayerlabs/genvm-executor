#!/usr/bin/env python3
"""Pre-commit guard: fail if dev-mode.nix is true or any hash is "test"."""

from __future__ import annotations

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
DEV_MODE_NIX = HERE / 'dev-mode.nix'
CURRENT_NIX = HERE / 'current.nix'


def _repo_root(start: Path) -> Path:
	for d in (start, *start.parents):
		if (d / '.genvm-monorepo-root').exists():
			return d
	raise RuntimeError(f'no .genvm-monorepo-root above {start}')


def _print_guide() -> None:
	try:
		guide = _repo_root(HERE) / 'docs/contributing/howto/committing/runners.md'
		print('=== docs/contributing/howto/committing/runners.md ===', file=sys.stderr)
		print(guide.read_text(), file=sys.stderr)
	except Exception:
		pass


errors: list[str] = []

if DEV_MODE_NIX.read_text().strip() == 'true':
	errors.append('runners/support/versions/dev-mode.nix is true')

if errors:
	for e in errors:
		print(f'error: {e}', file=sys.stderr)
	print(file=sys.stderr)
	_print_guide()
	sys.exit(1)
