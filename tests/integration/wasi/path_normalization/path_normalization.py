# { "Depends": "py-genlayer:test" }
from genlayer.vm import map_file, register_runner

code = b'# { "Depends": "py-genlayer:test" }\nmarker-12345\n'
rid = register_runner(code)

# A `.` component in a mapping destination must normalize away, not become a
# directory that hides the file from every guest path.
map_file(rid, 'file', '/dot/./mapped.txt')
with open('/dot/mapped.txt', 'rb') as f:
	print('dot-in-destination', f.read() == code)

# A `..` component in a guest path must resolve to the parent directory.
map_file(rid, 'file', '/parent/child/mapped.txt')
with open('/parent/child/../child/mapped.txt', 'rb') as f:
	print('parent-in-guest-path', f.read() == code)

exit(0)
