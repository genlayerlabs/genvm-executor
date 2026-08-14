# { "Depends": "py-genlayer:test" }
from genlayer.vm import map_file, register_runner

code = b'# { "Depends": "py-genlayer:test" }\nmarker-12345\n'
rid = register_runner(code)

# A mapping destination is contract-chosen, so its component count is too. The
# guest filesystem trie is dropped recursively, so a destination deeper than the
# bound must be refused rather than run the native stack out on teardown.
map_file(rid, 'file', '/' + '/'.join('d' for _ in range(20000)) + '/mapped.txt')

exit(0)
