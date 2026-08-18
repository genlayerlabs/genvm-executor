(module
	(import "softfloat" "f32_to_i32_trunc" (func $f32_to_i32_trunc (param f32) (result i32)))

	(func (export "_start")
		f32.const nan
		call $f32_to_i32_trunc
		drop
	)

	(memory $mem 1)
	(export "memory" (memory $mem))
)
