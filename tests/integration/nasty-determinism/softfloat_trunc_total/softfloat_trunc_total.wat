(module
	;; The other half of softfloat_trunc_trap: the saturating truncations are
	;; total operators, so no input may trap, and an in-range value must still
	;; come back from the trapping ones. This module traps only on a wrong
	;; answer, so reaching the end is the assertion.
	(@custom "genvm.runner.json" "{\"Seq\":[{\"Depends\":\"softfloat:test\"},{\"StartWasm\":\"file\"}]}")

	(import "softfloat" "f32_to_i32_trunc" (func $f32_to_i32_trunc (param f32) (result i32)))
	(import "softfloat" "f32_to_ui32_trunc" (func $f32_to_ui32_trunc (param f32) (result i32)))
	(import "softfloat" "f64_to_i64_trunc" (func $f64_to_i64_trunc (param f64) (result i64)))

	(import "softfloat" "f32_to_i32_sat" (func $f32_to_i32_sat (param f32) (result i32)))
	(import "softfloat" "f32_to_ui32_sat" (func $f32_to_ui32_sat (param f32) (result i32)))
	(import "softfloat" "f32_to_i64_sat" (func $f32_to_i64_sat (param f32) (result i64)))
	(import "softfloat" "f32_to_ui64_sat" (func $f32_to_ui64_sat (param f32) (result i64)))
	(import "softfloat" "f64_to_i32_sat" (func $f64_to_i32_sat (param f64) (result i32)))
	(import "softfloat" "f64_to_ui32_sat" (func $f64_to_ui32_sat (param f64) (result i32)))
	(import "softfloat" "f64_to_i64_sat" (func $f64_to_i64_sat (param f64) (result i64)))
	(import "softfloat" "f64_to_ui64_sat" (func $f64_to_ui64_sat (param f64) (result i64)))

	(func $want_i32 (param $got i32) (param $want i32)
		(if (i32.ne (local.get $got) (local.get $want)) (then unreachable)))

	(func $want_i64 (param $got i64) (param $want i64)
		(if (i64.ne (local.get $got) (local.get $want)) (then unreachable)))

	(func (export "_start")
		;; in range: the trapping operators still produce a value
		(call $want_i32 (call $f32_to_i32_trunc (f32.const -2.7)) (i32.const -2))
		(call $want_i32 (call $f32_to_ui32_trunc (f32.const 2.7)) (i32.const 2))
		(call $want_i64 (call $f64_to_i64_trunc (f64.const 1e18)) (i64.const 1000000000000000000))
		;; a negative that truncates to -0 is in range for an unsigned target
		(call $want_i32 (call $f32_to_ui32_trunc (f32.const -0.5)) (i32.const 0))

		;; NaN saturates to zero
		(call $want_i32 (call $f32_to_i32_sat (f32.const nan)) (i32.const 0))
		(call $want_i32 (call $f32_to_ui32_sat (f32.const nan)) (i32.const 0))
		(call $want_i64 (call $f32_to_i64_sat (f32.const nan)) (i64.const 0))
		(call $want_i64 (call $f32_to_ui64_sat (f32.const nan)) (i64.const 0))
		(call $want_i32 (call $f64_to_i32_sat (f64.const nan)) (i32.const 0))
		(call $want_i32 (call $f64_to_ui32_sat (f64.const nan)) (i32.const 0))
		(call $want_i64 (call $f64_to_i64_sat (f64.const nan)) (i64.const 0))
		(call $want_i64 (call $f64_to_ui64_sat (f64.const nan)) (i64.const 0))

		;; out of range saturates to the nearest representable bound
		(call $want_i32 (call $f32_to_i32_sat (f32.const 1e30)) (i32.const 2147483647))
		(call $want_i32 (call $f32_to_i32_sat (f32.const -1e30)) (i32.const -2147483648))
		(call $want_i32 (call $f32_to_ui32_sat (f32.const -1)) (i32.const 0))
		(call $want_i32 (call $f32_to_ui32_sat (f32.const 1e30)) (i32.const -1))
		(call $want_i64 (call $f64_to_i64_sat (f64.const 1e300)) (i64.const 9223372036854775807))
		(call $want_i64 (call $f64_to_ui64_sat (f64.const -1)) (i64.const 0))
	)

	(memory $mem 1)
	(export "memory" (memory $mem))
)
