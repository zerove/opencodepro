# C# AI 开发规范 (C# AI Spec)

## 1. AI 协同编码标准 (Coding Standards)
- **现代语法强制 (Modern C#)**：强制要求 AI 使用 C# 12 / .NET 8+ 特性。
  - *理由*：利用 `Primary Constructors`、`File-scoped namespaces` 和 `Raw string literals` 大幅减少样板代码。
- **LINQ 性能优化**：引导 AI 在处理大数据集时区分 `IEnumerable` 和 `IQueryable`，避免在内存中执行本应在数据库端完成的过滤。
- **异步模式统一**：强制要求所有异步方法遵循 `Async` 命名约定，并始终传递 `CancellationToken` 以支持优雅取消。

## 2. 定向提示策略 (Prompting Patterns)
- **内存分配优化**：*“请分析此关键路径代码中的内存分配，尝试使用 `Span<T>` 或 `Memory<T>` 来减少 GC 压力。”*
- **依赖注入审计**：*“请检查此类的构造函数，确保其符合依赖倒置原则，并建议最合适的 Service Lifetime (Transient/Scoped/Singleton)。”*
- **模式匹配增强**：*“请将此复杂的 if-else 逻辑重构为 C# 现代的 `switch` 表达式，利用模式匹配提高可读性。”*

## 3. AI 质量验收清单 (Validation Checklist)
- [ ] **异步完整性**：是否所有的 `await` 调用都正确处理了异常，且没有使用 `async void`？
- [ ] **LINQ 效率**：是否存在不必要的 `.ToList()` 调用导致过早加载数据？
- [ ] **资源释放**：所有实现 `IDisposable` 的对象是否都正确地包裹在 `using` 语句中？
- [ ] **空值处理**：是否利用了 `Nullable Reference Types` (NRT) 并在代码中明确处理了潜在的 null？

## ❌ 常见 AI 误区 (Anti-Patterns)
- **滥用 `async void`**：在非事件处理程序中使用 `async void`。$\rightarrow$ **正确做法**：始终返回 `Task` 或 `ValueTask`。
- **LINQ 性能陷阱**：在循环中重复调用 `.First()` 或 `.Any()` 导致多次遍历。$\rightarrow$ **正确做法**：先将结果物化到内存或使用更高效的查询。
- **过时同步阻塞**：使用 `.Result` 或 `.Wait()` 阻塞异步任务导致死锁。$\rightarrow$ **正确做法**：全程采用 `async/await` 异步链路。
