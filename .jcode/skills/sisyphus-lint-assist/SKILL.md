---
name: sisyphus-lint-assist
description: Sisyphus 专属代码检查与审计助手。在编写或修改代码时自动应用项目的严格门禁规则（如错误枚举、无 unwrap、LLM spawn 作用域标记等）。
---

# Sisyphus Lint Assist Skill

当用户需要编写或审查 Sisyphus 代码库的代码时，应用以下核心治理标准：

1. **错误处理**：
   - 使用 `thiserror` 定义具体的 Error enum（如 `CoreError`、`TeamError`、`TypesError`）。
   - **绝对禁止**在非测试代码（production code）中使用 `unwrap()` 或 `expect()`。

2. **异步与并发**：
   - 异步函数必须统一使用 `tokio` 和 `async-trait`。

3. **LLM Spawn 作用域边界**：
   - LLM 相关的 crate 中的非测试 spawn 必须走 `spawn_in_scope` / `attach_scope`，或附带清晰理由的 `// usage-scope: entry:<reason>` / `// usage-scope: no-llm:<reason>` marker。

4. **自动化审计门禁验证**：
   - 修改代码后，建议或运行相关审计脚本：
     - `python3 scripts/audit_todo.py`
     - `python3 scripts/audit_todo_semantic.py`
     - `python3 scripts/audit_version.py`
     - `python3 scripts/audit_doc_counts.py`
     - `python3 scripts/audit_links.py`
     - `python3 scripts/audit_llm_spawns.py`
