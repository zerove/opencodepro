(module
  (memory (export "memory") 1)
  ;; Returns the pointer it was handed, unchanged. The host reads a
  ;; NUL-terminated string back from that address, so echoing the pointer
  ;; makes the guest report whatever the host wrote there.
  (func (export "execute") (param i32) (result i32)
    local.get 0
  )
)