(module
  (memory (export "memory") 1)
  (data (i32.const 0) "Hello from WASM!\00")
  (func (export "execute") (result i32)
    (i32.const 0)
  )
)
