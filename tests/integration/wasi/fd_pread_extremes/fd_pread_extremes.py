# { "Depends": "py-genlayer:test" }
import os

from genlayer.vm import map_file, register_runner

code = b'# { "Depends": "py-genlayer:test" }\nmarker-12345\n'
rid = register_runner(code)

# `fd_pread` clamps the read length against the file size, and an offset at or
# past the end must read nothing rather than index the backing slice out of
# bounds.
map_file(rid, 'file', '/pread/mapped.txt')
with open('/pread/mapped.txt', 'rb') as f:
	fd = f.fileno()
	print('inside', os.pread(fd, 6, 2) == code[2:8])
	print('at-end', os.pread(fd, 4, len(code)))
	print('past-end', os.pread(fd, 4, len(code) + 1))
	print('max', os.pread(fd, 4, 2**63 - 1))
	print('offset-kept', os.lseek(fd, 0, os.SEEK_CUR))

exit(0)
