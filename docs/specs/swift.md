# Swift AI 开发规范 (Swift AI Spec)

## 1. AI 协同编码标准 (Coding Standards)
- **值类型优先 (Value Types First)**：强制 AI 优先使用 `struct` 和 `enum` 而非 `class`。
  - *理由*：利用 Swift 的值语义减少共享状态带来的 Bug，并提升内存访问性能。
- **声明式 UI 模式 (Declarative UI)**：在开发 SwiftUI 应用时，强制 AI 遵循单向数据流 (`@State` $\rightarrow$ `View` $\rightarrow$ `Action` $\rightarrow$ `@State`)。
- **强类型可选值 (Strong Optionals)**：禁止使用强行解包 (`!`)。强制要求 AI 使用 `if let` 或 `guard let` 进行安全的解包。

## 2. 定向提示策略 (Prompting Patterns)
- **内存管理审计**：*“请检查此闭包中是否存在循环引用 (Strong Reference Cycle)，并建议在何处使用 `[weak self]` 或 `[unowned self]`。”*
- **并发模型转换**：*“请将此旧的 GCD (Grand Central Dispatch) 代码重构为现代的 `async/await` 和 `Task` 结构化并发模型。”*
- **协议导向编程 (POP)**：*“请通过定义 `Protocol` 来解耦此功能，而非使用类继承，以提高代码的可测试性和灵活性。”*

## 3. AI 质量验收清单 (Validation Checklist)
- [ ] **解包安全性**：代码中是否出现了危险的 `!` 操作符？
- [ ] **状态流向**：SwiftUI 的状态更新是否触发了不必要的全局刷新？
- [ ] **资源占用**：闭包捕获列表是否正确处理了内存引用？
- [ ] **API 惯例**：函数命名是否符合 Swift 的 API 设计指南（例如：使用 `is` 或 `has` 作为布尔值前缀）？

## ❌ 常见 AI 误区 (Anti-Patterns)
- **过度依赖 Class**：习惯性地使用 `class` 来定义简单的数据模型。$\rightarrow$ **正确做法**：优先使用 `struct`，仅在需要引用语义或继承时使用 `class`。
- **强制解包陷阱**：使用 `!` 来处理 Optional 变量以简化代码。$\rightarrow$ **正确做法**：始终使用 `guard let` 或 `if let` 进行安全绑定。
- **闭包循环引用**：在闭包中直接使用 `self` 而不标注引用类型。$\rightarrow$ **正确做法**：明确使用 `[weak self]` 防止内存泄漏。
