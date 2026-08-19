# { "Depends": "py-genlayer:test" }
import genlayer as gl
from genlayer.vm import map_file

# The storage-read permission was removed; map_file works without an `r` grant.
try:
	map_file(gl.vm.RunnerIDOps.CONTRACT, 'file', '/mapped')
	print('mapped')
except Exception as e:
	print(f'forbidden: {type(e).__name__}')
exit(0)
