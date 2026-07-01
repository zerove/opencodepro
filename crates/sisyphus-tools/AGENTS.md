# Sisyphus Tools Agent Knowledge Base

**OVERVIEW**
Implementation of the tool ecosystem for the Sisyphus cognitive engine.

**STRUCTURE**
- `src/`: Tool implementations and core registry.
- `src/build/`: Language-specific build system integrations.
- `src/mcp/`: Model Context Protocol client and transport.
- `src/team/`: Multi-agent team orchestration logic.
- `src/custom_tool/`: Dynamic loading of external tool definitions.
- `tests/`: Integration tests for tool chains and team coordination.

**WHERE TO LOOK**
- System/Shell: `bash.rs`, `git.rs`, `file.rs`
- AI/Cognition: `llm.rs`, `oracle.rs`, `librarian.rs`, `explore.rs`, `model_router.rs`
- Dev/Build: `lsp.rs`, `benchmark.rs`, `test_gen.rs`, `src/build/`
- Integration: `github.rs`, `web.rs`, `bridge.rs`
- Management: `agent_registry.rs`, `tool_registry.rs`, `delegate.rs`, `src/team/`
- Specialized: `security.rs`, `diagnose.rs`, `auto_fix.rs`, `bug_fixer.rs`

**CONVENTIONS**
- Tools implement `async-trait` for execution.
- Error handling via `thiserror` (propagated to `TypesError` or `CoreError`).
- Minimal custom types used for LSP to avoid OOM.

**COMMANDS**
- `cargo test -p sisyphus-tools` - Run all tool tests.
- `cargo test -p sisyphus-tools --test agent_team_e2e` - Run team orchestration tests.
