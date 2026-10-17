;; Guest fixture for the `CustomToolExecutor` ABI.
;;
;; Unlike `echo_args.wat`, which targets the host-write ABI (the host appends the
;; arguments at the end of linear memory and passes the offset), this module
;; targets the `alloc` ABI: the host calls `alloc(size)`, writes the JSON
;; arguments at the returned pointer, then calls `execute(ptr)`.
;;
;; `execute` hands the pointer straight back, which turns this module into a
;; mirror: a test can assert that the bytes the host wrote are exactly the bytes
;; the host reads back. Any break in the alloc/write/execute/read-back sequence
;; shows up as a mismatch instead of being hidden by the guest.
;;
;; `alloc` deliberately fills the memory *after* the allocation with 0xAA.
;; Fresh linear memory is zero-filled, which would hide a missing NUL
;; terminator: without one, a host read-back would still stop at exactly
;; args.len() because the next byte happened to be zero. With 0xAA there, a
;; missing terminator makes the read-back run past the argument, so the test can
;; actually observe the terminator's presence.
(module
  (memory (export "memory") 1)

  ;; Bump allocator. Starts well past offset 0 so a stray zero offset cannot be
  ;; mistaken for a valid allocation.
  (global $next (mut i32) (i32.const 1024))

  (func (export "alloc") (param $size i32) (result i32)
    (local $ptr i32)
    (local $i i32)
    (local $limit i32)
    (local.set $ptr (global.get $next))
    (local.set $limit
      (i32.add (local.get $ptr) (i32.add (local.get $size) (i32.const 256))))

    ;; Poison the whole span, then hand back the start of it.
    (block $filled
      (loop $fill
        (br_if $filled (i32.ge_u (local.get $i) (i32.const 256)))
        (i32.store8
          (i32.add (local.get $ptr) (local.get $i))
          (i32.const 0xAA))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $fill)))

    (global.set $next (local.get $limit))
    (local.get $ptr))

  (func (export "execute") (param $ptr i32) (result i32)
    (local.get $ptr))
)
