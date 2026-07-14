# Python AI 开发规范 (Python AI Spec)

## 1. AI 协同编码标准 (Coding Standards)
- **类型提示 (Type Hinting)**：强制要求 AI 在所有函数签名中使用 `typing` 模块。
  - *理由*：类型提示是 AI 理解代码逻辑最直接的线索，能显著降低 AI 在后续修改时的幻觉率。
- **文档字符串 (Docstrings)**：要求 AI 使用 Google 风格的 docstrings。
- **异步优先**：在处理 IO 密集型任务时，优先引导 AI 使用 `asyncio`。
- **依赖管理**：优先使用 `pyproject.toml` (Poetry/Pdm) 而非简单的 `requirements.txt`。

## 2. 定向提示策略 (Prompting Patterns)
- **逻辑验证**：当 AI 生成复杂逻辑时，使用提示词：*“请为该逻辑编写 3 个覆盖边缘情况的单元测试，并解释为什么这些情况是关键的。”*
- **性能优化**：针对 Python 性能瓶 bottleneck，使用：*“请分析该函数的复杂度，并尝试使用 itertools 或 numpy 进行向量化优化。”*
- **类型增强**：*“请将此函数升级为强类型定义，并使用 mypy 检查可能的类型冲突。”*

## 3. AI 质量验收清单 (Validation Checklist)
- [ ] **类型完整性**：是否所有函数都有完整的类型标注？
- [ ] **异常处理**：是否使用了具体的 Exception 而非泛泛的 `Exception`？
- [ ] **资源管理**：所有文件/网络连接是否都使用了 `with` 上下文管理器？
- [ ], **Pythonic 程度**：代码是否使用了列表推导式、生成器等 Pythonic 特性，而非冗长的 C 风格循环？

## ❌ 常见 AI 误区 (Anti-Patterns)
- **C 风格循环**：使用 `for i in range(len(list))` 遍历。$\rightarrow$ **正确做法**：使用 `for item in list` 或 `for i, item in enumerate(list)`。
- **字符串拼接低效**：在循环中使用 `+` 拼接字符串。$\rightarrow$ **正确做法**：使用 `"".join(list)`。
- ** mutable 默认参数**：在函数定义中使用 `def func(a=[])`。$\rightarrow$ **正确做法**：使用 `def func(a=None)` 然后在内部初始化。
