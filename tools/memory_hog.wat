(module
  (memory (export "memory") 1)
  (func (export "execute") (param i32) (result i32)
    (memory.grow 1000)
    i32.const 0
  )
)
