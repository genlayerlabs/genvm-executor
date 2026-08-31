# { "Depends": "py-genlayer:test" }
import genlayer as gl


class Contract(gl.contract.Contract):
	@gl.public.write
	def main(self):
		def validate(result):
			return (
				isinstance(result, gl.vm.VMError)
				and result.public_code == 'out_of receipt nondet_output'
			)

		result = gl.vm.run_nondet(
			lambda: 'x' * 1024,
			validate,
			catch_vm_error=True,
		)
		assert isinstance(result, gl.vm.VMError)
		print(result.public_code)
