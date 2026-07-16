# { "Depends": "py-genlayer:test" }
from genlayer.vm import register_runner

# register_runner is granted to the initial VM without a top-level permission flag.
try:
	register_runner(b'# { "Depends": "py-genlayer:test" }\nx\n')
	print('registered')
except Exception as e:
	print(e)
exit(0)
