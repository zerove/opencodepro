# Java AI 开发规范 (Java AI Spec)

## 1. AI 协同编码标准 (Coding Standards)
- **现代语法优先 (Modern Java)**：强制要求 AI 使用 Java 17+ 特性。
  - *理由*：使用 `Records` 取代冗长的 POJO，使用 `Text Blocks` 优化 SQL/JSON 字符串，显著降低 AI 生成代码的行数和复杂度。
- **响应式与函数式风格**：引导 AI 优先使用 `Stream API` 和 `Optional` 处理集合与空值，避免过多的 `if (obj != null)` 嵌套。
- **Spring Boot 惯例**：在开发 Web 应用时，强制 AI 遵循分层架构 (`Controller` $\rightarrow$ `Service` $\rightarrow$ `Repository`)，并严格使用构造函数注入而非 `@Autowired` 字段注入。

## 2. 定向提示策略 (Prompting Patterns)
- **设计模式对齐**：*“请使用 [策略模式/工厂模式] 实现此功能，并确保符合 Spring 的 Bean 生命周期管理。”*
- **并发安全审计**：*“请分析该代码在多线程环境下的线程安全性，特别是关于 `ConcurrentHashMap` 和 `Atomic` 变量的使用。”*
- **性能优化分析**：*“请对比当前实现与使用 `ParallelStream` 或 `CompletableFuture` 的性能差异，并给出适用场景建议。”*

## 3. AI 质量验收清单 (Validation Checklist)
- [ ] **空指针防护**：是否正确使用了 `Optional` 且没有在 `Optional.get()` 前进行检查？
- [ ] **异常处理**：是否定义了全局异常处理器 (`@ControllerAdvice`)，而非在业务代码中随处 `try-catch`？
- [ ] **资源管理**：所有 IO 操作是否使用了 `try-with-resources`？
- [ ] **依赖注入**：是否所有依赖都通过构造函数注入以方便进行单元测试？

## ❌ 常见 AI 误区 (Anti-Patterns)
- **过度设计**：为了展示能力而引入不必要的复杂设计模式（如在简单逻辑中强行使用访问者模式）。$\rightarrow$ **正确做法**：优先选择最简单的实现，仅在必要时扩展。
- **Java 8 停留在过去**：继续使用旧的 POJO 和冗长的 Getter/Setter。$\rightarrow$ **正确做法**：使用 Java 14+ 的 `Records`。
- **滥用 Stream API**：将极简单的循环写成复杂的 Stream 链，导致可读性下降。$\rightarrow$ **正确做法**：当 Stream 链超过 3 层且逻辑复杂时，考虑回归到增强型 for 循环。
