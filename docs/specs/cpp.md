# C++ AI 开发规范 (C++ AI Spec)

## 1. AI 协同编码标准 (Coding Standards)
- **现代标准强制 (Modern C++)**：强制要求 AI 遵循 C++17/20 标准。禁止使用 `malloc/free`，强制使用 `std::unique_ptr` 和 `std::shared_ptr`。
- **资源管理 (RAII)**：强制所有资源（文件、锁、内存）必须通过 RAII 模式管理。
- **类型安全**：引导 AI 使用 `std::optional` 和 `std::variant` 取代魔数或空指针来表示缺失/多种状态。

## 2. 定向提示策略 (Prompting Patterns)
- **内存所有权审计**：*“请分析此对象的生命周期，详细说明谁拥有所有权，以及在何处被销毁，以确保没有内存泄漏或悬空指针。”*
- **复杂度与性能权衡**：*“请提供两种实现方案：一种是快速原型的简单实现，另一种是针对缓存局部性和内存对齐优化的高性能实现。”*
- **现代特性重构**：*“请将此 C++98 风格的代码重构为 C++20 风格，利用 `concepts` 和 `ranges` 库来增强可读性和类型安全。”*

## 3. AI 质量验收清单 (Validation Checklist)
- [ ] **内存安全**：是否完全消除了裸指针 (`raw pointer`) 的 `delete` 操作？
- [ ] **边界检查**：所有的容器访问是否都进行了边界检查或使用了 `.at()`？
- [ ] **常量正确性**：所有不修改状态的函数是否都正确标注了 `const`？
- [ ] **并发安全**：在多线程环境下，所有的共享变量是否都被 `std::mutex` 或 `std::atomic` 保护？

## ❌ 常见 AI 误区 (Anti-Patterns)
- **手动内存管理**：依然尝试使用 `new/delete` 而非智能指针。$\rightarrow$ **正确做法**：全面使用 `std::unique_ptr` 或 `std::shared_ptr`。
- **忽略常量正确性**：成员函数忘记标注 `const`，导致无法传递常量引用。$\rightarrow$ **正确做法**：严格执行 `const` 传播。
- **过时的 C 风格数组**：使用 `int arr[10]` 而非 `std::array` 或 `std::vector`。$\rightarrow$ **正确做法**：优先使用 STL 容器以获得边界安全和迭代器支持。
