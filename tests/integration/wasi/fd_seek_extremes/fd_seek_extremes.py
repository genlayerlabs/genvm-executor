# { "Depends": "py-genlayer:test" }
import os

from genlayer.vm import map_file, register_runner

code = b'# { "Depends": "py-genlayer:test" }\nmarker-12345\n'
rid = register_runner(code)

# `fd_seek` clamps an out-of-range offset to the file bounds. The extremes are
# what the arithmetic has to survive: negating `i64::MIN` overflows, and the
# executor profile aborts on overflow rather than wrapping.
map_file(rid, 'file', '/seek/mapped.txt')
with open('/seek/mapped.txt', 'rb') as f:
	fd = f.fileno()
	print('min-cur', os.lseek(fd, -(2**63), os.SEEK_CUR))
	print('min-set', os.lseek(fd, -(2**63), os.SEEK_SET))
	print('max-cur', os.lseek(fd, 2**63 - 1, os.SEEK_CUR) == len(code))

exit(0)
