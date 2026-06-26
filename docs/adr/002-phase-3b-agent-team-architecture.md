# ADR-002: Phase 3b Multi-Agent Team Architecture

- **Status**: Proposed
- **Date**: 2026-06-25
- **Driver**: Sisyphus Phase 3b (代理-模型映射 / 团队模式 / 结果聚合)

## Context

Phase 1a–3a implemented a single-model tool dispatch architecture:
- **One** `DynamicLlmClient` wrapping one `Config.llm` — all agent-like tools (librarian, metis, momus, scribe, security) share the same model
- **WorkOrchestrator** runs independent `WorkUnit` closures in parallel, collects `Vec<WorkReport>` without cross-agent synthesis
- No concept of agent identity, capability declaration, or model affinity

Phase 3b requirements:
1. Different agents need different models (e.g., metis → o3-mini for deep reasoning, librarian → gemma for fast retrieval)
2. Dynamic agent registration and discovery (what agents exist, what they do)
3. Team-level orchestration: decompose goal → assign sub-tasks → synthesize results

## Decision

We introduce three new abstractions layered on the existing infrastructure:

### 1. Per-Agent Model Override (`LlmConfig` extension)

Add a `HashMap<String, AgentLlmOverride>` to `LlmConfig` that lets any agent override the default endpoint/model/api_key:

```toml
[llm]
endpoint = "http://localhost:8000/v1/chat/completions"
model = "gpt-4o"

[llm.agents.metis]
model = "o3-mini"
endpoint = "https://api.openai.com/v1"

[llm.agents.librarian]
model = "gemma-4-31b-it"
```

Rust types (in `sisyphus-types`):

```rust
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AgentLlmOverride {
    pub model: String,
    pub endpoint: Option<String>,
    pub api_key: Option<String>,
}

// Add to existing LlmConfig:
pub struct LlmConfig {
    pub api_key: String,
    pub endpoint: String,
    pub model: String,
    #[serde(default)]
    pub agents: HashMap<String, AgentLlmOverride>,
}
```

`DynamicLlmClient::get_params_for(agent_id: &str)` returns the override if present, else falls back to the default.

### 2. AgentRegistry (`sisyphus-tools`)

A registry that maps agent IDs to their capabilities and model config:

```rust
pub struct AgentInfo {
    pub display_name: String,
    pub description: String,
    pub tools: Vec<String>,        // tool_id prefixes this agent handles
    pub max_concurrency: usize,
    pub model_override: Option<AgentLlmOverride>,
}

pub struct AgentRegistry {
    agents: RwLock<HashMap<String, AgentInfo>>,
    llm_config: Arc<RwLock<Config>>,
}
```

Key methods:
- `register(id, info)` — add or update an agent
- `get(id) -> Option<AgentInfo>` — look up
- `get_llm_client(id) -> Box<dyn LlmClient>` — create a per-agent DynamicLlmClient
- `list_agents() -> Vec<(String, AgentInfo)>` — discovery
- `resolve_tool(tool_id) -> Option<String>` — find which agent handles a tool

Built-in agents are registered at startup from a hardcoded table:

| Agent ID | Tools | Default Model |
|---|---|---|
| `librarian` | `librarian/*` | gemma-4-31b-it |
| `metis` | `metis/*` | o3-mini |
| `momus` | `momus/*` | claude-sonnet-4 |
| `scribe` | `scribe/*` | gpt-4o |
| `security` | `security/*` | gpt-4o |

### 3. TeamOrchestrator

A higher-level orchestrator that wraps `WorkOrchestrator` with task decomposition and result synthesis:

```rust
pub struct TeamGoal {
    pub id: String,
    pub objective: String,
    pub context: Option<String>,
    pub required_agents: Vec<String>,   // empty = auto-select
}

pub struct TaskNode {
    pub id: String,
    pub agent_id: String,
    pub tool_id: String,
    pub args: serde_json::Value,
    pub depends_on: Vec<String>,       // task IDs that must complete first
}

pub struct TeamPlan {
    pub goal_id: String,
    pub tasks: Vec<TaskNode>,
}

pub struct TaskResult {
    pub task_id: String,
    pub agent_id: String,
    pub success: bool,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
    pub duration_ms: u128,
}

pub struct TeamReport {
    pub goal_id: String,
    pub success: bool,
    pub task_results: Vec<TaskResult>,
    pub synthesis: Option<String>,
    pub total_duration_ms: u128,
}

pub enum TeamEvent {
    Planning { goal_id: String },
    TaskStarted { task_id: String, agent_id: String },
    TaskCompleted(TaskResult),
    Synthesis { report: String },
    Failed { error: String },
}

pub struct TeamOrchestrator {
    registry: Arc<AgentRegistry>,
    executor: Arc<dyn ToolExecutor>,
    planner_llm: Arc<dyn LlmClient>,     // LLM used for decomposition/synthesis
}
```

**Control flow:**

```
TeamOrchestrator::run(goal)
  │
  ├─ 1. plan(): LLM decomposes goal → TeamPlan (task DAG)
  │       [uses planner_llm — typically metis or a strong model]
  │
  ├─ 2. execute(): Walk task DAG in topological order
  │       For each ready task:
  │         - resolve agent via AgentRegistry
  │         - create per-agent LLM client if needed
  │         - dispatch via executor.execute(tool_id, args)
  │       Parallel via WorkOrchestrator, respecting depends_on
  │
  ├─ 3. synthesize(): LLM combines task_results → synthesis string
  │       [uses planner_llm or the most capable agent]
  │
  └─ 4. return TeamReport { results, synthesis, metrics }
```

### Integration with Existing Infrastructure

- **Config**: `AgentLlmOverride` uses existing `Arc<RwLock<Config>>` hot-reload mechanism
- **Executor**: `TeamOrchestrator.execute()` calls `ExecutorRouter::execute(tool_id, args)` — no change needed
- **Hooks**: `BuiltinToolExecutor` already applies hooks; team orchestrator doesn't bypass them
- **WorkOrchestrator**: TeamOrchestrator uses `WorkOrchestrator::run_all_with_events()` for parallel task execution with progress streaming

### Implementation Plan (Incremental)

| Step | What | Files |
|---|---|---|
| 1 | Add `AgentLlmOverride`, extend `LlmConfig` | `sisyphus-types/src/lib.rs` |
| 2 | Add `DynamicLlmClient::get_params_for()` | `sisyphus-tools/src/llm.rs` |
| 3 | Create `AgentRegistry` module | `sisyphus-tools/src/agent_registry.rs` |
| 4 | Create `TeamOrchestrator` module | `sisyphus-tools/src/team.rs` |
| 5 | Register built-in agents at startup | `sisyphus-bridge/src/main.rs` |
| 6 | Add gRPC endpoints for agent discovery | `sisyphus-bridge/src/main.rs` |
| 7 | Tests | Each module's `#[cfg(test)]` |

## Consequences

### Positive
- Backward compatible: existing single-llm config continues to work unchanged
- Incremental: each step can land independently
- Hot-reload works for agent overrides via existing `ConfigStore` mechanism
- TeamOrchestrator builds on proven `WorkOrchestrator` primitives
- No changes to hook system or executor dispatch

### Negative
- AgentRegistry introduces mutable global state (mitigated by `RwLock`)
- Task dependency resolution adds complexity over flat `run_all()`
- Planning/synthesis LLM calls add latency

### Risks
- LLM-based task decomposition may produce suboptimal plans — start with manual plan authoring API
- Agent-LLM binding latency: creating per-agent clients on every call may be expensive; consider caching
- Task DAG cycle detection needed if LLM generates plans
