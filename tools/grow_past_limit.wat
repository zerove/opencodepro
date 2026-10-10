;; Grows linear memory by 64 pages — well past the default 10-page limit.
;;
;; **The negative case** for `grow_within_limit.wat`. The module declares no
;; maximum of its own, so nothing but the sandbox's `max_memory_pages` can stop
;; this grow. Identical in every other respect to that fixture, which is what
;; makes the pair discriminating: same engine, same shape, only the requested
;; size differs.
;;
;; Two spellings matter here, and both were learned the hard way:
;;
;; * `(memory.grow N)` is **wrong**. `memory.grow` takes its page count from the
;;   stack; under the multi-memory proposal that wasmtime enables by default,
;;   the bare immediate is read as a *memory index*, and the module is rejected
;;   with `unknown memory N` at compile time. The correct form is
;;   `(i32.const N) (memory.grow 0)`.
;; * The `execute` result is a **pointer**, not the value. The host reads the
;;   `i32` as an address into linear memory and returns the null-terminated
;;   string found there, so the grow result is only observable if the module
;;   renders it into memory and returns that address.
;;
;; A refused grow does not trap; it yields -1. This module renders whichever
;; value it gets as a decimal string at address 64, so under the default
;; 10-page budget the host sees "-1".
(module
  (memory (export "memory") 1)
  (data (i32.const 64) "0\00")

  ;; Render $n as a decimal string at $ptr; return the start address.
  (func $itoa (param $n i32) (param $ptr i32) (result i32)
    (local $start i32)
    (local.set $start (local.get $ptr))
    (if (i32.lt_s (local.get $n) (i32.const 0))
      (then
        (i32.store8 (local.get $ptr) (i32.const 45)) ;; '-'
        (local.set $ptr (i32.add (local.get $ptr) (i32.const 1)))
        (local.set $n (i32.sub (i32.const 0) (local.get $n)))))
    (if (i32.ge_u (local.get $n) (i32.const 10))
      (then
        (local.set $ptr
          (call $itoa
            (i32.div_u (local.get $n) (i32.const 10))
            (local.get $ptr)))))
    (i32.store8 (local.get $ptr)
      (i32.add (i32.const 48) (i32.rem_u (local.get $n) (i32.const 10))))
    (i32.store8 (i32.add (local.get $ptr) (i32.const 1)) (i32.const 0))
    (local.get $start))

  (func (export "execute") (result i32)
    (local $prev i32)
    (i32.const 64)
    (memory.grow 0)
    (local.set $prev)
    (call $itoa (local.get $prev) (i32.const 64))
    (drop)
    (i32.const 64))
)