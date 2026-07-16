(module
	(type $void_fn (func))

	;; A minimal raw-wasm contract: parsed via the Raw WASM path into a
	;; `{ "StartWasm": "file" }` runner with no Depends/With, so its whole runner
	;; tree is a single load action — the main chain:deploy runner itself. That
	;; makes the load-action arithmetic exactly hand-computable: one `charged`
	;; record of `RUNNER_LOAD_COST + size`, where `size` == the raw wasm length.
	(func (export "_start") (type $void_fn))

	(memory $mem 1)
	(export "memory" (memory $mem))
)
