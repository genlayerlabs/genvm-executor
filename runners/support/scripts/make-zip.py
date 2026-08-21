import importlib._bootstrap_external
import importlib.util
import json
import os
import struct
import zlib
from pathlib import Path

src_dir = Path('.')

deduce_src = [x for x in src_dir.iterdir() if x.name not in ['scripts', 'env-vars']]
if len(deduce_src) != 1:
	raise Exception(f'Invalid structure {deduce_src}')

base_dir = deduce_src[0]
print(f'base_dir={base_dir}')

all_files: dict[str, bytes] = {}


def add_compiled(name: str, py_src: bytes):
	opt_level = 0
	pyc_name = importlib.util.cache_from_source(name, optimization='')
	code = compile(py_src, name, 'exec', dont_inherit=True, optimize=opt_level)
	source_hash = importlib.util.source_hash(py_src)

	bytecode = importlib._bootstrap_external._code_to_hash_pyc(
		code,
		source_hash,
		False,
	)

	add_file(pyc_name, bytecode, skip_pyc=False)


def check_entry_name(name: str):
	"""
	The executor's entry-name rules, enforced here so a rejected name is a build
	failure rather than a malformed-runner error at load time. Names are also
	required to be ASCII: the emitted flags claim CP437, not UTF-8.
	"""
	if not name.isascii():
		raise Exception(f'entry name {name!r} is not ASCII')
	if name.startswith('/') or '\\' in name:
		raise Exception(f'entry name {name!r} has a leading slash or a backslash')
	if any(part in ('', '.', '..') for part in name.split('/')):
		raise Exception(f'entry name {name!r} has an invalid path component')


def add_file(name: str, contents: bytes, skip_pyc=True):
	if skip_pyc and (name.endswith('.pyc') or name.endswith('.pyo')):
		return
	if name in all_files:
		raise KeyError(f'EEXISTS: {name}')
	if name.endswith('/'):
		return  # skip dir
	check_entry_name(name)

	if name.endswith('.py'):
		add_compiled(name, contents)

	if name == 'runner.json':
		new_contents = (
			json.dumps(json.loads(contents), separators=(',', ':'), sort_keys=True) + '\n'
		).encode('utf-8')
		contents = new_contents

	all_files[name] = contents


for path in base_dir.glob('**/*'):
	if not path.is_file():
		continue

	add_file(str(path.relative_to(base_dir)), path.read_bytes())

assert len(all_files) != 0
assert 'runner.json' in all_files, f'files are {all_files}'

# Every byte of the zip is spelled out here rather than delegated to `zipfile`,
# which derives `create_system` and `external_attr` from the build host and
# picks its own zip64 thresholds -- none of which a content hash may depend on.
LOCAL_SIGNATURE = 0x04034B50
CENTRAL_SIGNATURE = 0x02014B50
END_SIGNATURE = 0x06054B50
VERSION_MADE_BY = 0x0014
VERSION_NEEDED = 20
FLAGS = 0
METHOD_STORED = 0
DOS_TIME = 0
DOS_DATE = 0x0021  # 1980-01-01
LOCAL_HEADER_LEN = 30

if len(all_files) > 0xFFFF:
	raise Exception(f'{len(all_files)} entries need zip64, which is not emitted')

local_records: list[bytes] = []
central_records: list[bytes] = []
offset = 0

for name, contents in sorted(all_files.items(), key=lambda x: x[0]):
	name_bytes = name.encode('utf-8')
	size = len(contents)
	if size > 0xFFFFFFFF or offset > 0xFFFFFFFF:
		raise Exception(f'{name} needs zip64, which is not emitted')

	# The local and central records are packed from this single description, so
	# they cannot disagree about what the entry is.
	shared = (
		VERSION_NEEDED,
		FLAGS,
		METHOD_STORED,
		DOS_TIME,
		DOS_DATE,
		zlib.crc32(contents),
		size,
		size,
		len(name_bytes),
	)

	local_records.append(
		struct.pack(
			'<I5H3I2H',
			LOCAL_SIGNATURE,
			*shared,
			0,  # extra field length
		)
		+ name_bytes
		+ contents
	)
	central_records.append(
		struct.pack(
			'<IH5H3IH4H2I',
			CENTRAL_SIGNATURE,
			VERSION_MADE_BY,
			*shared,
			0,  # extra field length
			0,  # comment length
			0,  # disk number start
			0,  # internal attributes
			0,  # external attributes
			offset,
		)
		+ name_bytes
	)

	offset += LOCAL_HEADER_LEN + len(name_bytes) + size

central_directory = b''.join(central_records)

zip_contents = (
	b''.join(local_records)
	+ central_directory
	+ struct.pack(
		'<I4H2IH',
		END_SIGNATURE,
		0,  # this disk
		0,  # disk with the central directory
		len(all_files),
		len(all_files),
		len(central_directory),
		offset,
		0,  # comment length
	)
)

Path(os.environ['out']).write_bytes(zip_contents)
