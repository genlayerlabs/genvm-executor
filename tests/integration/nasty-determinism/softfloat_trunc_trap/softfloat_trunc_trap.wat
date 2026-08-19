(module
	;; `i32.trunc_f32_s` is a partial operator: for a NaN the wasm spec gives it
	;; no result at all, and the execution rule turns that into a trap. Contracts
	;; reach it through softfloat, which must therefore trap too rather than hand
	;; back a sentinel.
	;;
	;; The runner is declared in-module: a raw wasm module's `runner.json` comes
	;; from this custom section, so no zip and no prepare step are needed.
	(@custom "genvm.runner.json" "{\"Seq\":[{\"Depends\":\"softfloat:test\"},{\"StartWasm\":\"file\"}]}")

	(import "softfloat" "f32_to_i32_trunc" (func $f32_to_i32_trunc (param f32) (result i32)))

	(func (export "_start")
		f32.const nan
		call $f32_to_i32_trunc
		drop
	)

	(memory $mem 1)
	(export "memory" (memory $mem))
)
