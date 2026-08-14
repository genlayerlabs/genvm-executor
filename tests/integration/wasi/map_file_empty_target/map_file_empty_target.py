# { "Depends": "py-genlayer:test" }
from genlayer.vm import map_file, register_runner

code = b'# { "Depends": "py-genlayer:test" }\nmarker-12345\n'
rid = register_runner(code)

# A destination that normalizes to zero components ('', '/', '///', '/.', '.')
# names no file. The rejection is a malformed_runner trap, so it terminates
# execution and is not catchable here -- but it must still reach the host as a
# receipt, not as an internal error.
map_file(rid, 'file', '/')

exit(0)
