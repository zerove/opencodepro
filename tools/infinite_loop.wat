(module
  (func (export "execute") (param i32) (result i32)
    (local i32)
    loop $loop
      local.get 0
      i32.const 1
      i32.add
      local.set 0
      br $loop
    end
    unreachable
  )
)
