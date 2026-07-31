import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).parents[1] / 'wasmtime_maintenance.py'
SPEC = importlib.util.spec_from_file_location('wasmtime_maintenance', SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class MaintenanceTests(unittest.TestCase):
	def test_load_lines_sorts_and_preserves_policy(self):
		with tempfile.TemporaryDirectory() as directory:
			path = Path(directory) / 'config.json'
			path.write_text(
				json.dumps(
					{
						'schema_version': 1,
						'lines': {
							'v0.10': {'policy': 'monthly'},
							'v0.2': {'policy': 'security-only'},
						},
					}
				)
			)
			lines = MODULE.load_lines(path)

		self.assertEqual([line.name for line in lines], ['v0.2', 'v0.10'])
		self.assertEqual(lines[0].refs['development'], 'v0.2-dev')
		self.assertEqual(lines[0].refs['release'], 'v0.2.x')
		self.assertEqual(lines[1].policy, 'monthly')

	def test_parse_pin_reads_custom_commit_and_lock_version(self):
		manifest = json.dumps(
			{
				'repos': {
					'executor/third-party/wasmtime': {
						'commit': 'a' * 40,
						'patches': 6,
					}
				}
			}
		)
		lock = '''
[[package]]
name = "wasmtime"
version = "42.0.2"

[[package]]
name = "wasmtime-environ"
version = "42.0.2"
'''

		pin = MODULE.parse_pin('v0.3', 'development', 'v0.3-dev', manifest, lock)

		self.assertEqual(pin.version, '42.0.2')
		self.assertEqual(pin.commit, 'a' * 40)
		self.assertEqual(pin.ref, 'v0.3-dev')

	def test_parse_pin_rejects_ambiguous_versions(self):
		manifest = json.dumps(
			{
				'repos': {
					'executor/third-party/wasmtime': {
						'commit': 'b' * 40,
					}
				}
			}
		)
		lock = '''
[[package]]
name = "wasmtime"
version = "41.0.0"

[[package]]
name = "wasmtime"
version = "42.0.2"
'''

		with self.assertRaisesRegex(ValueError, 'exactly one'):
			MODULE.parse_pin('v0.3', 'development', 'v0.3-dev', manifest, lock)

	def test_marker_round_trips(self):
		key = 'advisory:v0.3:GHSA-abcd-1234-5678'
		body = f'prefix\n{MODULE.marker(key)}\nsuffix'
		self.assertEqual(MODULE.marker_key(body), key)

	def test_advisory_aliases_prefer_ghsa(self):
		advisory = MODULE.advisory_from_osv(
			{
				'id': 'RUSTSEC-2026-0114',
				'aliases': ['CVE-2026-44216', 'GHSA-p8xm-42r7-89xg'],
				'summary': 'panic',
			}
		)

		self.assertEqual(advisory.id, 'GHSA-p8xm-42r7-89xg')
		self.assertEqual(
			advisory.url,
			'https://osv.dev/vulnerability/GHSA-p8xm-42r7-89xg',
		)

	def test_advisories_body_groups_findings_and_names_branch_flow(self):
		line = MODULE.Line(name='v0.3', policy='monthly')
		pin = MODULE.Pin(
			line='v0.3',
			channel='development',
			ref='v0.3-dev',
			version='42.0.2',
			commit='c' * 40,
		)
		advisory = MODULE.Advisory(
			id='GHSA-test',
			summary='test advisory',
			url='https://example.invalid',
		)

		body = MODULE.advisories_body(
			line,
			{advisory.id: (advisory, [pin])},
			'genvm-owner',
		)

		self.assertIn('pr/v0.3/<manager-feature>', body)
		self.assertIn('v0.3-dev', body)
		self.assertIn('v0.3.x', body)
		self.assertIn('@genvm-owner', body)
		self.assertIn('<!-- wasmtime-maintenance:advisories:v0.3 -->', body)


if __name__ == '__main__':
	unittest.main()
