# { "Depends": "py-genlayer:test" }
import genlayer as gl

# mapping into /vm/ is forbidden; the runtime gl_call path must reject it just
# like the InitAction MapFile path does (see runners/map-vm). The rejection is a
# malformed_runner trap, so it terminates execution and is not catchable here.
gl.vm.map_file(gl.vm.RunnerIDOps.CONTRACT, 'file', '/vm/evil.py')
exit(0)
