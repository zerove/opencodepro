# Go AI 开发规范 (Go AI Spec)

## 1. AI 协同编码标准 (Coding Standards)
- **极致简约 (Simplicity First)**：强制 AI 遵循 "Effective Go" 风格。拒绝过度工程，禁止在不需要时使用复杂的泛型或深度嵌套的接口。
- **显式错误处理 (Explicit Error Handling)**：禁止使用 `panic` 处理可预见的错误。强制 AI 采用 `if err != nil { return ... }` 的标准模式。
- **并发原语正确性**：在涉及并发时，强制 AI 优先考虑 `channel` 进行通信，而非过度依赖 `sync.Mutex`。
- **接口最小化**：引导 AI 定义极小的接口 (Small Interfaces)，遵循“接受接口，返回结构体”的原则。

## 2. 定向提示策略 (Prompting Patterns)
- **并发模式审计**：*“请分析此 Goroutine 的生命周期，是否存在泄露风险？请通过 context.Context 实现优雅退出。”*
- **切片/Map 性能分析**：*“请检查此处是否有频繁的内存分配，尝试使用 `make` 预分配容量以优化性能。”*
- **错误链构建**：*“请使用 `fmt.Errorf` 和 `%w` 为此错误添加上下文包裹，确保调用方能追溯根因。”*

## 3. AI 质量验收清单 (Validation Checklist)
- [ ] **错误处理完整性**：每一个可能返回 `error` 的函数调用是否都被显式处理了？
- [ ] **Goroutine 安全**：所有启动的 Goroutine 是否都有明确的退出机制？
- [ ] **零值可用性**：结构体的零值状态是否可用且安全？
- [ ] **依赖极简主义**：是否引入了不必要的第三方库？能否使用标准库实现？

## ❌ 常见 AI 误区 (Anti-Patterns)
- **忽略错误**：使用 `_ = someFunc()` 忽略错误返回。$\rightarrow$ **正确做法**：必须显式处理 `err`。
- **Goroutine 泄露**：在循环中启动 Goroutine 而没有停止信号。$\rightarrow$ **正确做法**：使用 `context.Context` 或 `channel` 传递退出信号。
- **过度接口化**：为每个结构体都定义一个接口，即便只有一个实现。$\rightarrow$ **正确做法**：遵循“接受接口，返回结构体”原则。
