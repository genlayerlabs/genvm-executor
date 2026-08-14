;; `fd_write` sums the ciovec lengths into a u32 to report how much it wrote.
;; The buffers may overlap, so that sum is not bounded by the guest's memory:
;; two ciovecs covering nearly all of a 4 GiB memory add up past u32::MAX. The
;; total must be rejected before anything is written, not accumulated into an
;; overflowing counter.
(module
	(import "wasi_snapshot_preview1" "fd_write"
		(func $fd_write (param i32 i32 i32 i32) (result i32)))

	(memory $mem 1)
	(export "memory" (memory $mem))

	;; iovecs live at 0..16, nwritten at 32, messages at 64+, and the first
	;; ciovec points at 1 MiB, well inside the grown memory
	(data (i32.const 64) "rejected\n")
	(data (i32.const 80) "accepted\n")
	(data (i32.const 96) "no-memory\n")

	(func $say (param $ptr i32) (param $len i32)
		(i32.store (i32.const 0) (local.get $ptr))
		(i32.store (i32.const 4) (local.get $len))
		(drop (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 32)))
	)

	(func (export "_start")
		(local $step i32)
		(local $total i32)
		(local $first i32)

		;; grow to the largest memory the limiter allows: by 1024 pages while
		;; that works, then halving the step down to one page
		(local.set $step (i32.const 1024))
		(block $done
			(loop $outer
				(br_if $done (i32.eqz (local.get $step)))
				(block $shrink
					(loop $grow
						(br_if $shrink
							(i32.eq (memory.grow (local.get $step)) (i32.const -1)))
						(br $grow)
					)
				)
				(local.set $step (i32.div_u (local.get $step) (i32.const 2)))
				(br $outer)
			)
		)

		;; bytes of memory, and the shortest first ciovec whose sum with a
		;; whole-memory second ciovec reaches 2^32
		(local.set $total (i32.mul (memory.size) (i32.const 65536)))
		(local.set $first (i32.sub (i32.const 0) (local.get $total)))

		;; the first ciovec is written before the second one is added in, so a
		;; broken executor writes `first` bytes to stdout before it aborts.
		;; That is bounded by how much memory the limiter granted; refuse to run
		;; at all if the grant came out small enough to make it huge.
		(if (i32.gt_u (local.get $first) (i32.const 268435456))
			(then
				(call $say (i32.const 96) (i32.const 10))
				(return)
			)
		)

		(i32.store (i32.const 0) (i32.const 1048576))
		(i32.store (i32.const 4) (local.get $first))
		(i32.store (i32.const 8) (i32.const 0))
		(i32.store (i32.const 12) (local.get $total))

		(if (i32.eqz (call $fd_write
				(i32.const 1) (i32.const 0) (i32.const 2) (i32.const 32)))
			(then (call $say (i32.const 80) (i32.const 9)))
			(else (call $say (i32.const 64) (i32.const 9)))
		)
	)
)
