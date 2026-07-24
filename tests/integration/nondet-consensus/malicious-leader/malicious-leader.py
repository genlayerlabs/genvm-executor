# { "Depends": "py-genlayer:test" }
import genlayer as gl


class Contract(gl.contract.Contract):
	def __init__(self, name: str, comment: str):
		print(comment)

		def do_vm_error():
			exit(2)

		def do_user_error():
			gl.vm.UserError.immediate({'foo': 'bar'})

		def do_return():
			return 11

		fns = {
			'do_vm_error': do_vm_error,
			'do_user_error': do_user_error,
			'do_return': do_return,
		}

		gl.eq_principle.strict_eq(fns[name])
