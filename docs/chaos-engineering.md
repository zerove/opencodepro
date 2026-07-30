# Chaos Engineering in Sisyphus

## Overview

Sisyphus includes a built-in chaos engineering framework that injects controlled
faults into the system to test resilience, verify error-handling paths, and
validate the self-programming auto-repair pipeline.

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                   Bridge (gRPC)                      │
│  ┌──────────────────────────────────────────────┐   │
│  │  maybe_inject_chaos_fault()                  │   │
│  │  ├─ Random delay injection (50–500ms)        │   │
│  │  └─ CHAOS_ERROR fault injection              │   │
│  │       └─ chaos_fault_counter (AtomicU64)     │   │
│  │            └─ if > 3 → Auto-fix pipeline     │   │
│  └──────────────────────────────────────────────┘   │
│                                                      │
│  ┌──────────────────────────────────────────────┐   │
│  │  ChaosRunner (core crate)                    │   │
│  │  ├─ FaultType enum (LlmTimeout, ToolCrash…) │   │
│  │  ├─ maybe_inject(op_name) → Option<Error>   │   │
│  │  └─ start/stop experiment lifecycle          │   │
│  └──────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────┐
│           Auto-Fix Pipeline (Direction 4)            │
│                                                      │
│  1. Bridge injects CHAOS_ERROR fault                 │
│  2. Fault counter incremented                        │
│  3. If total > 3, background task spawned:           │
│     a. Record MetaLearningSample to MetaLearner      │
│     b. Call SelfProgrammingManager::resolve_gap()    │
│        with synthetic ToolGap describing the fault   │
│  4. SelfProgrammingManager generates, compiles,      │
│     deploys, and tests a new tool                    │
└─────────────────────────────────────────────────────┘
```

## Components

### 1. Bridge-level Fault Injection (`bridge_service.rs`)

The `MySisyphusBridge` has a `maybe_inject_chaos_fault()` method that:

- Checks if chaos is enabled (`chaos.enabled = true` in Config)
- Uses a probability gate (`chaos.default_fault_probability`)
- 50/50 split between:
  - **Delay injection**: async sleep for 50–500ms (transparent — execution proceeds)
  - **Error injection**: returns `CHAOS_ERROR: injected fault` without executing

**New in Direction 4 — Auto-Fix Pipeline:**

When the bridge injects a CHAOS_ERROR fault:
1. `chaos_fault_counter` is incremented
2. `chaos_fault_type_counters` records the fault type
3. If `chaos_fault_counter > 3`, the auto-fix is triggered:
   - `chaos_auto_fix_triggered` flag is set (for test verification)
   - A background `tokio::spawn` task runs:
     - Records a `MetaLearningSample` in `Kernel.meta_learner`
     - Calls `SelfProgrammingManager::resolve_gap()` with a synthetic `ToolGap`
       describing the required chaos-robustness improvement

### 2. Core-level ChaosRunner (`chaos_testing.rs`)

The `sisyphus_core::ChaosRunner` provides DAG-level fault injection:

- `start_experiment(experiment)` — begins a chaos experiment
- `maybe_inject(op_name)` — returns `CoreError` if a fault should occur
- `stop_experiment()` — ends the experiment and returns results

See [`crates/sisyphus-core/src/chaos_testing.rs`](../crates/sisyphus-core/src/chaos_testing.rs).

### 3. Fault Types

| FaultType | Description | Bridge Level | Core Level |
|-----------|-------------|-------------|------------|
| CHAOS_ERROR | Synthetic error returned before execution | ✓ | ✗ |
| LlmTimeout | Simulates an LLM timeout | ✗ | ✓ |
| LlmGarbage | LLM returns unparseable response | ✗ | ✓ |
| ToolCrash | Tool execution crash | ✗ | ✓ |
| ToolHang | Tool execution never returns | ✗ | ✓ |
| StorageWriteFailure | Storage write failure | ✗ | ✓ |
| StorageReadFailure | Storage read failure | ✗ | ✓ |
| NetworkPartition | Network partition | ✗ | ✓ |
| RandomDelay | Random latency injection | ✓ (bridge) | ✓ (core) |

## Configuration

```toml
[chaos]
enabled = true
default_fault_probability = 0.1
```

## Testing

### Integration Tests

Located in `crates/sisyphus-bridge/tests/bridge_integration_test.rs`:

| Test | Description |
|------|-------------|
| `test_chaos_enabled_does_not_crash` | 100% fault prob, 20 calls — verify no panics |
| `test_chaos_faults_occur_with_high_probability` | 90% fault prob, verify rate > 25% |
| `test_chaos_disabled_no_faults` | Default config — all calls succeed |
| `test_chaos_auto_triggers_fix` | **Direction 4**: 5 faults → auto-fix triggered |

### ChaosRunner Tests

Located in `crates/sisyphus-core/src/chaos_testing.rs`:

| Test | Description |
|------|-------------|
| `test_chaos_runner_enable_disable` | Enable/disable lifecycle |
| `test_chaos_runner_probability` | Probability gate behavior |
| `test_chaos_runner_targeted_operations` | Targeted fault injection |
| `test_chaos_runner_error_rate` | Error rate calculation |

## Auto-Fix Pipeline (Direction 4)

### Trigger Conditions

- `chaos.enabled = true` (config)
- `chaos_fault_counter > 3` (i.e. 4th or later fault)

### Pipeline Steps

1. **Fault Injection**: `maybe_inject_chaos_fault()` returns `CHAOS_ERROR`
2. **Counting**: `chaos_fault_counter` and `chaos_fault_type_counters` updated
3. **Background Task** (tokio::spawn):
   - Records fault as `MetaLearningSample` in `Kernel.meta_learner`
   - Constructs synthetic `ToolGap` with:
     - `tool_id`: "chaos_auto_repair"
     - `required_capability`: "Handle recurring chaos faults…"
   - Calls `SelfProgrammingManager::resolve_gap(gap)`
4. **Resolution**: SelfProgrammingManager generates, compiles, verifies, deploys,
   and registers a new tool to handle the failure mode

### Verification

```rust
let count = bridge.chaos_fault_count();
assert!(count >= 5, "Expected at least 5 faults, got {count}");
assert!(bridge.chaos_auto_fix_was_triggered(),
        "Auto-fix should have been triggered after >3 faults");
```

## References

- `crates/sisyphus-bridge/src/bridge_service.rs` — Bridge-level injection + auto-fix
- `crates/sisyphus-bridge/src/grpc_handlers.rs` — gRPC handler calling `maybe_inject_chaos_fault`
- `crates/sisyphus-bridge/tests/bridge_integration_test.rs` — Integration tests
- `crates/sisyphus-core/src/chaos_testing.rs` — Core ChaosRunner + FaultType
- `crates/sisyphus-core/src/self_programming_manager.rs` — SelfProgrammingManager
