# ADR-006: Configuration System

**Status:** Accepted
**Date:** 2026-07-30
**Author:** Sisyphus Team

## Context

Sisyphus has many components (LLM, cache, sandbox, metrics, chaos, MCP, registry) each with their own configuration parameters. We need a consistent configuration system.

## Decision

### Configuration Sources (ordered by priority)

1. **Default values** — hardcoded in each sub-config's `Default` impl
2. **Config file** — TOML file deserialized into `sisyphus_types::Config`
3. **Environment variables** — `SISYPHUS_*` prefix, highest priority

### Design

```rust
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    pub llm: LlmConfig,
    pub cognitive_loop: CognitiveLoopConfig,
    pub executor: ExecutorConfig,
    pub cache: CacheConfig,
    pub metrics: MetricsConfig,
    pub sandbox: SandboxConfig,
    pub mcp: McpConfig,
    pub registry: RegistryConfig,
    pub jcode: JcodeConfig,
    pub chaos: ChaosConfig,
    pub cluster: ClusterConfig,
    pub jwt: JwtConfig,
    pub auth_token: String,
    pub bridge_address: String,
    pub http_address: String,
}
```

Each sub-config lives under `config/` in `sisyphus-types` (`sub.rs` for most; `llm.rs` for LLM).
Environment variables are applied in `Config::apply_env_overrides()`.

### Env Var Convention

- Prefix: `SISYPHUS_`
- Path: `SISYPHUS_<SECTION>_<FIELD>` (e.g. `SISYPHUS_CACHE_L1_CAPACITY`)
- Nested: `SISYPHUS_<SECTION>_<SUB>_<FIELD>` (e.g. `SISYPHUS_COGNITIVE_LOOP_ADAPTIVE_DEPTH_DECAY`)

### Config File Loading

```rust
let path = std::env::var("SISYPHUS_CONFIG").unwrap_or_else(|_| "config.toml".into());
let mut config = Config::from_file(&path)?;
config.apply_env_overrides();
```

### Placeholder Resolution

`{env:VAR}` and `{env:VAR|default}` placeholders are resolved in string fields via `Config::resolve_env_placeholders()`.

## Consequences

**Positive:**
- Single source of truth in `Config` struct
- Clear override order (file < env)
- Each deployment can override subset of config

**Negative:**
- `apply_env_overrides()` needs manual maintenance when adding new fields
- No hot-reload for services: `Config::watch()` / `ConfigWatcher` exists in `config/mod.rs`, but the bridge does not consume it (restart required)