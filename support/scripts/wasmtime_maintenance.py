#!/usr/bin/env python3
"""
Monitor Wasmtime advisories and schedule owned rebase reviews.

The scheduled workflow runs from the repository's default branch, but executor
release lines move independently. This tool therefore reads each configured
`vX-dev` and `vX.x` ref through the GitHub API. It only reconciles issues; it
never creates branches or changes protected refs.
"""

from __future__ import annotations

import argparse
import base64
import dataclasses
import datetime as dt
import json
import os
import re
import sys
import tomllib
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_CONFIG = ROOT / '.github' / 'wasmtime-maintenance.json'
THIRD_PARTY_CONFIG = '.git-third-party/config.json'
CARGO_LOCK = 'executor/Cargo.lock'
WASMTIME_REPO_KEY = 'executor/third-party/wasmtime'
OSV_QUERY_URL = 'https://api.osv.dev/v1/query'
LABEL = 'wasmtime-maintenance'
MARKER_RE = re.compile(r'<!-- wasmtime-maintenance:([^>]+) -->')
VALID_LINE_RE = re.compile(r'v\d+\.\d+')
VALID_POLICIES = {'monthly', 'security-only'}


@dataclasses.dataclass(frozen=True)
class Line:
	name: str
	policy: str

	@property
	def refs(self) -> dict[str, str]:
		return {'development': f'{self.name}-dev', 'release': f'{self.name}.x'}


@dataclasses.dataclass(frozen=True)
class Pin:
	line: str
	channel: str
	ref: str
	version: str
	commit: str


@dataclasses.dataclass(frozen=True)
class Advisory:
	id: str
	summary: str
	url: str


def load_lines(path: Path = DEFAULT_CONFIG) -> list[Line]:
	raw = json.loads(path.read_text())
	if raw.get('schema_version') != 1:
		raise ValueError(f'{path}: unsupported schema_version')
	lines = raw.get('lines')
	if not isinstance(lines, dict) or not lines:
		raise ValueError(f'{path}: lines must be a non-empty object')

	result = []
	for name, conf in lines.items():
		if not VALID_LINE_RE.fullmatch(name):
			raise ValueError(f'{path}: invalid executor line {name!r}')
		policy = conf.get('policy') if isinstance(conf, dict) else None
		if policy not in VALID_POLICIES:
			raise ValueError(f'{path}: invalid policy for {name}: {policy!r}')
		result.append(Line(name=name, policy=policy))
	return sorted(result, key=lambda line: tuple(map(int, line.name[1:].split('.'))))


def parse_pin(line: str, channel: str, ref: str, manifest: str, lock: str) -> Pin:
	repos = json.loads(manifest).get('repos', {})
	wasmtime = repos.get(WASMTIME_REPO_KEY)
	if not isinstance(wasmtime, dict):
		raise ValueError(f'{ref}: missing {WASMTIME_REPO_KEY} pin')
	commit = wasmtime.get('commit')
	if not isinstance(commit, str) or not re.fullmatch(r'[0-9a-f]{40}', commit):
		raise ValueError(f'{ref}: invalid Wasmtime commit {commit!r}')

	packages = tomllib.loads(lock).get('package', [])
	versions = {
		package.get('version')
		for package in packages
		if package.get('name') == 'wasmtime' and isinstance(package.get('version'), str)
	}
	if len(versions) != 1:
		raise ValueError(f'{ref}: expected exactly one Wasmtime version, found {versions}')
	return Pin(
		line=line,
		channel=channel,
		ref=ref,
		version=versions.pop(),
		commit=commit,
	)


def marker(key: str) -> str:
	return f'<!-- wasmtime-maintenance:{key} -->'


def marker_key(body: str | None) -> str | None:
	match = MARKER_RE.search(body or '')
	return match.group(1) if match else None


def advisory_from_osv(item: dict[str, Any]) -> Advisory:
	identifiers = {item['id'], *item.get('aliases', [])}
	canonical = next(
		iter(sorted(identifier for identifier in identifiers if identifier.startswith('GHSA-'))),
		None,
	)
	if canonical is None:
		canonical = next(
			iter(
				sorted(
					identifier
					for identifier in identifiers
					if identifier.startswith('RUSTSEC-')
				)
			),
			item['id'],
		)
	return Advisory(
		id=canonical,
		summary=item.get('summary') or 'Published Wasmtime advisory',
		url=f'https://osv.dev/vulnerability/{canonical}',
	)


def request_json(
	url: str,
	*,
	method: str = 'GET',
	token: str | None = None,
	payload: dict[str, Any] | None = None,
) -> Any:
	data = None if payload is None else json.dumps(payload).encode()
	headers = {
		'Accept': 'application/vnd.github+json',
		'User-Agent': 'genvm-wasmtime-maintenance',
	}
	if data is not None:
		headers['Content-Type'] = 'application/json'
	if token:
		headers['Authorization'] = f'Bearer {token}'
		headers['X-GitHub-Api-Version'] = '2022-11-28'
	request = urllib.request.Request(url, data=data, headers=headers, method=method)
	try:
		with urllib.request.urlopen(request, timeout=30) as response:
			return json.load(response)
	except urllib.error.HTTPError as error:
		detail = error.read().decode(errors='replace')
		raise RuntimeError(f'{method} {url}: HTTP {error.code}: {detail}') from error


class GitHub:
	def __init__(self, repository: str, token: str, *, dry_run: bool = False):
		if not re.fullmatch(r'[^/]+/[^/]+', repository):
			raise ValueError(f'invalid GITHUB_REPOSITORY {repository!r}')
		self.repository = repository
		self.token = token
		self.dry_run = dry_run

	def api(
		self,
		path: str,
		*,
		method: str = 'GET',
		payload: dict[str, Any] | None = None,
	) -> Any:
		return request_json(
			f'https://api.github.com{path}',
			method=method,
			token=self.token,
			payload=payload,
		)

	def file(self, ref: str, path: str) -> str:
		quoted_path = urllib.parse.quote(path, safe='/')
		query = urllib.parse.urlencode({'ref': ref})
		data = self.api(f'/repos/{self.repository}/contents/{quoted_path}?{query}')
		if data.get('encoding') != 'base64':
			raise RuntimeError(f'{ref}:{path}: GitHub returned unsupported encoding')
		return base64.b64decode(data['content']).decode()

	def ensure_label(self) -> None:
		if self.dry_run:
			return
		try:
			self.api(
				f'/repos/{self.repository}/labels',
				method='POST',
				payload={
					'name': LABEL,
					'color': 'b60205',
					'description': 'Automated Wasmtime advisory and rebase tracking',
				},
			)
		except RuntimeError as error:
			if 'HTTP 422' not in str(error):
				raise

	def issues(self) -> list[dict[str, Any]]:
		result = []
		page = 1
		while True:
			query = urllib.parse.urlencode(
				{'state': 'all', 'labels': LABEL, 'per_page': 100, 'page': page}
			)
			batch = self.api(f'/repos/{self.repository}/issues?{query}')
			result.extend(issue for issue in batch if 'pull_request' not in issue)
			if len(batch) < 100:
				return result
			page += 1

	def upsert_issue(
		self,
		existing: dict[str, Any] | None,
		*,
		title: str,
		body: str,
		owner: str | None,
		reopen: bool,
	) -> None:
		if self.dry_run:
			action = 'update' if existing else 'create'
			print(f'[dry-run] {action} issue: {title}')
			return

		if existing is None:
			payload: dict[str, Any] = {'title': title, 'body': body, 'labels': [LABEL]}
			if owner:
				payload['assignees'] = [owner]
			try:
				self.api(
					f'/repos/{self.repository}/issues',
					method='POST',
					payload=payload,
				)
			except RuntimeError as error:
				if owner and 'HTTP 422' in str(error):
					print(
						f'::warning::Could not assign Wasmtime issue to {owner}; '
						'creating it unassigned'
					)
					payload.pop('assignees')
					self.api(
						f'/repos/{self.repository}/issues',
						method='POST',
						payload=payload,
					)
				else:
					raise
			return

		payload = {}
		if existing.get('title') != title:
			payload['title'] = title
		if existing.get('body') != body:
			payload['body'] = body
		if reopen and existing.get('state') == 'closed':
			payload['state'] = 'open'
		if owner and owner not in {item['login'] for item in existing.get('assignees', [])}:
			payload['assignees'] = [owner]
		if payload:
			self.api(
				f'/repos/{self.repository}/issues/{existing["number"]}',
				method='PATCH',
				payload=payload,
			)

	def close_issue(self, existing: dict[str, Any], *, title: str) -> None:
		if existing.get('state') == 'closed':
			return
		if self.dry_run:
			print(f'[dry-run] close issue: {title}')
			return
		self.api(
			f'/repos/{self.repository}/issues/{existing["number"]}',
			method='PATCH',
			payload={'state': 'closed'},
		)


def osv_advisories(pin: Pin) -> list[Advisory]:
	queries = [
		{
			'package': {'name': 'wasmtime', 'ecosystem': 'crates.io'},
			'version': pin.version,
		},
		{'commit': pin.commit},
	]
	found: dict[str, Advisory] = {}
	for query in queries:
		page_token = None
		while True:
			payload = dict(query)
			if page_token:
				payload['page_token'] = page_token
			response = request_json(OSV_QUERY_URL, method='POST', payload=payload)
			for item in response.get('vulns', []):
				advisory = advisory_from_osv(item)
				found[advisory.id] = advisory
			page_token = response.get('next_page_token')
			if not page_token:
				break
	return sorted(found.values(), key=lambda advisory: advisory.id)


def advisories_body(
	line: Line,
	affected: dict[str, tuple[Advisory, list[Pin]]],
	owner: str | None,
) -> str:
	rows = []
	for advisory, pins in sorted(affected.values(), key=lambda item: item[0].id):
		summary = advisory.summary.replace('|', '\\|').replace('\n', ' ')
		refs = ', '.join(f'`{pin.ref}`' for pin in pins)
		rows.append(f'| [{advisory.id}]({advisory.url}) | {summary} | {refs} |')
	owner_text = f'@{owner}' if owner else '**not configured** (`WASMTIME_REBASE_OWNER`)'
	return f"""{marker(f'advisories:{line.name}')}
## Published Wasmtime advisories

| Advisory | Summary | Affected refs |
|---|---|---|
{chr(10).join(rows)}

**Owner:** {owner_text}

This issue is continuously reconciled. A finding can be closed as not
applicable only after the owner records which disabled feature or local patch
removes the affected path.

### Required branch flow

- Fix the line through a manager feature branch and executor mirror
  `pr/{line.name}/<manager-feature>`.
- Target the executor PR at `{line.name}-dev`; do not push the protected branch.
- If `{line.name}.x` is affected, promote the fix through the standing release
  gate after development and cross-repository checks pass.

### Verification

- [ ] confirm the advisory applies to GenVM's enabled features and local patches
- [ ] rebase the Wasmtime patch stack onto a fixed upstream release
- [ ] replay every third-party patch from a clean checkout
- [ ] run the complete executor test suite
- [ ] run upstream `cargo test -p wasmtime --tests`
- [ ] run determinism and fingerprint regressions
- [ ] link the manager and executor PRs here
"""


def review_body(
	line: Line,
	pins: list[Pin],
	owner: str | None,
	month: str,
	latest_release: str,
) -> str:
	rows = '\n'.join(
		f'| `{pin.ref}` | {pin.channel} | `{pin.version}` | `{pin.commit[:12]}` |'
		for pin in pins
	)
	owner_text = f'@{owner}' if owner else '**not configured** (`WASMTIME_REBASE_OWNER`)'
	return f"""{marker(f'review:{line.name}:{month}')}
## Scheduled Wasmtime rebase review

- **Executor line:** `{line.name}`
- **Review month:** `{month}`
- **Latest upstream release:** `{latest_release}`
- **Owner:** {owner_text}

| Ref | Channel | Version | Upstream commit |
|---|---|---:|---|
{rows}

This is an owned review, not an automatic upgrade. GenVM carries
consensus-critical Wasmtime patches, so the owner must assess the candidate
release and record either a rebase PR or a reason to retain the current pin.

### Checklist

- [ ] review Wasmtime release notes and published advisories
- [ ] decide whether this line should rebase this month
- [ ] audit every GenVM patch for upstream overlap or semantic changes
- [ ] use `pr/{line.name}/<manager-feature>` for the executor mirror
- [ ] target `{line.name}-dev` and use the standing gate for `{line.name}.x`
- [ ] attach clean patch-replay and full test evidence
- [ ] link the manager and executor PRs, or record the no-change decision
"""


def pins_for_line(github: GitHub, line: Line) -> list[Pin]:
	result = []
	for channel, ref in line.refs.items():
		result.append(
			parse_pin(
				line.name,
				channel,
				ref,
				github.file(ref, THIRD_PARTY_CONFIG),
				github.file(ref, CARGO_LOCK),
			)
		)
	return result


def latest_wasmtime_release(github: GitHub) -> str:
	release = github.api('/repos/bytecodealliance/wasmtime/releases/latest')
	return release.get('tag_name') or 'unknown'


def reconcile(config: Path, *, dry_run: bool, today: dt.date | None = None) -> int:
	repository = os.environ.get('GITHUB_REPOSITORY', '')
	token = os.environ.get('GITHUB_TOKEN', '')
	if not repository:
		raise ValueError('GITHUB_REPOSITORY is required')
	if not token and not dry_run:
		raise ValueError('GITHUB_TOKEN is required unless --dry-run is used')
	owner = os.environ.get('WASMTIME_REBASE_OWNER') or None
	if owner is None:
		print(
			'::warning::WASMTIME_REBASE_OWNER is not configured; '
			'maintenance issues will be unassigned'
		)

	github = GitHub(repository, token, dry_run=dry_run)
	github.ensure_label()
	existing = {
		key: issue
		for issue in github.issues()
		if (key := marker_key(issue.get('body'))) is not None
	}
	latest_release = latest_wasmtime_release(github)
	current_month = (today or dt.datetime.now(dt.UTC).date()).strftime('%Y-%m')

	for line in load_lines(config):
		pins = pins_for_line(github, line)
		affected: dict[str, tuple[Advisory, list[Pin]]] = {}
		for pin in pins:
			print(f'checking {pin.ref}: Wasmtime {pin.version} ({pin.commit[:12]})')
			for advisory in osv_advisories(pin):
				entry = affected.setdefault(advisory.id, (advisory, []))
				entry[1].append(pin)

		key = f'advisories:{line.name}'
		if affected:
			github.upsert_issue(
				existing.get(key),
				title=(
					f'[Wasmtime][{line.name}] {len(affected)} published '
					'advisories affect pinned refs'
				),
				body=advisories_body(line, affected, owner),
				owner=owner,
				reopen=True,
			)
		elif key in existing:
			github.close_issue(
				existing[key],
				title=f'[Wasmtime][{line.name}] published advisories cleared',
			)

		if line.policy == 'monthly':
			key = f'review:{line.name}:{current_month}'
			github.upsert_issue(
				existing.get(key),
				title=f'[Wasmtime][{line.name}] {current_month} scheduled rebase review',
				body=review_body(line, pins, owner, current_month, latest_release),
				owner=owner,
				reopen=False,
			)
	return 0


def audit_local() -> int:
	line = os.environ.get('GITHUB_BASE_REF', 'local').removesuffix('-dev')
	pin = parse_pin(
		line,
		'proposed',
		os.environ.get('GITHUB_HEAD_REF', 'working-tree'),
		(ROOT / THIRD_PARTY_CONFIG).read_text(),
		(ROOT / CARGO_LOCK).read_text(),
	)
	advisories = osv_advisories(pin)
	if not advisories:
		print(f'Wasmtime {pin.version} ({pin.commit[:12]}): no published advisories found')
		return 0
	for advisory in advisories:
		print(f'::error title={advisory.id}::{advisory.summary} ({advisory.url})')
	return 1


def parser() -> argparse.ArgumentParser:
	result = argparse.ArgumentParser(description=__doc__)
	result.add_argument('--config', type=Path, default=DEFAULT_CONFIG)
	subcommands = result.add_subparsers(dest='command', required=True)
	reconcile_parser = subcommands.add_parser('reconcile')
	reconcile_parser.add_argument('--dry-run', action='store_true')
	subcommands.add_parser('audit-local')
	return result


def main(argv: list[str] | None = None) -> int:
	args = parser().parse_args(argv)
	if args.command == 'audit-local':
		return audit_local()
	return reconcile(args.config, dry_run=args.dry_run)


if __name__ == '__main__':
	try:
		sys.exit(main())
	except (RuntimeError, ValueError, OSError, json.JSONDecodeError, tomllib.TOMLDecodeError) as error:
		print(f'::error::{error}', file=sys.stderr)
		sys.exit(1)
