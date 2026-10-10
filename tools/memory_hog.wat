;; Grows linear memory by 1000 pages — far beyond any sandbox limit.
;;
;; The `(memory.grow 1000)` spelling this file used before was **wrong** on two
;; counts, and the resulting test was vacuous:
;;
;; * `memory.grow` takes its page count from the stack. Under the multi-memory
;;   proposal that wasmtime enables by default, the bare immediate is parsed as
;;   a *memory index*, so the module was rejected with `unknown memory 1000` at
;;   compile time. It had never run.
;; * The old `test_execute_memory_exhaustion` asserted only `is_err()`, and a
;;   compile failure is an error too, so it passed for a reason unrelated to
;;   memory limits.
;;
;; The body below is otherwise identical to `grow_past_limit.wat`: the same
;; `$itoa` helper, the same pointer ABI, and the same "render the grow result as
;; a decimal string" contract. Only the page count differs. A refused grow does
;; not trap; it yields -1, so this reports "-1" under any sane page budget.
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
    (i32.const 1000)
    (memory.grow 0)
    (local.set $prev)
    (call $itoa (local.get $prev) (i32.const 64))
    (drop)
    (i32.const 64))
)