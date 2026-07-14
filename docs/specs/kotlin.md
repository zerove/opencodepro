# Kotlin AI 开发规范 (Kotlin AI Spec)

## 1. AI 协同编码标准 (Coding Standards)
- **极致简洁化 (Idiomatic Kotlin)**：强制 AI 停止编写“Java 风格的 Kotlin”。优先使用 `Data Classes`、`Extension Functions` 和 `Scope Functions` (`let`, `apply`, `also`, `run`)。
- **空安全至上 (Null Safety)**：禁止使用 `!!` 强制解包。强制 AI 使用 `?.` (Safe Call)、`?:` (Elvis Operator) 或 `requireNotNull()`。
- **结构化并发 (Structured Concurrency)**：在异步编程中，强制 AI 使用 `CoroutineScope` 和 `async/await`，严禁在生产代码中直接使用 `GlobalScope`。

## 2. 定向提示策略 (Prompting Patterns)
- **函数式重构**：*“请将此循环逻辑重构为 Kotlin 集合操作符 (如 `flatMap`, `associateBy`, `partition`)，使其更具声明性。”*
- **协程上下文分析**：*“请分析此代码在切换 `Dispatchers.Main` 和 `Dispatchers.IO` 时的正确性，确保没有在主线程执行耗时操作。”*
- **DSL 构建引导**：*“请利用 Kotlin 的 Type-Safe Builders 为此配置项构建一个简单的 DSL，提高配置文件的可读性。”*

## 3. AI 质量验收清单 (Validation Checklist)
- [ ] **空安全审计**：代码中是否依然存在 `!!` 操作符？
- [ ] **内存泄漏预防**：在 Android 开发中，协程是否绑定到了正确的 `lifecycleScope` 或 `viewModelScope`？
- [ ] **不可变性检查**：是否尽可能地使用了 `val` 而非 `var`？
- [ ] **命名惯例**：函数和变量是否遵循 Kotlin 的驼峰命名法，且扩展函数命名是否直观？

## ❌ 常见 AI 误区 (Anti-Patterns)
- **Java 式 Kotlin**：编写冗长的 Getter/Setter 或使用 `for (i in 0..list.size-1)`。$\rightarrow$ **正确做法**：利用 `data class` 和 `forEachIndexed`。
- **滥用 `!!` 强制解包**：在面对可选类型时为了方便直接使用双感叹号。$\rightarrow$ **正确做法**：使用 `?.let { ... }` 或 `?: throw ...`。
- **协程作用域错误**：在非协程环境中通过 `GlobalScope.launch` 启动异步任务。$\rightarrow$ **正确做法**：始终在明确的 `CoroutineScope` 中启动，确保生命周期可控。
