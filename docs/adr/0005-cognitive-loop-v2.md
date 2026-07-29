# ADR 005: Cognitive Loop v2 — Adaptive Depth, Branch Evaluation, and Anomaly Detection

## Status
Implemented (2026-07-29)

## Context
The original cognitive loop (v1) used a fixed `max_iterations` ceiling (default 10) and a static `convergence_threshold` (default 0.15). This was simple but wasteful — it either ran too many iterations past convergence or stopped too early on complex tasks. Additionally, v1 had no runtime anomaly detection, used a single execution path with no parallel exploration, and had no mechanism to detect degradation mid-execution.

The complexity of real-world SOP graphs varies widely: a trivial file-rename task converges in 1–2 iterations, while a multi-step refactor with branching dependencies can require 20+. A one-size-fits-all iteration cap was insufficient.

## Decision
We introduced three new components in `cognitive_loop_v2.rs`:

### 1. AdaptiveDepthController
Replaces the fixed `max_iterations` with a confidence-based adaptive depth controller.

**How it works:**
- Tracks confidence per iteration and computes convergence velocity over a sliding window (default 3 iterations).
- Uses `ComplexityAnalyzer` from `sisyphus-intel` to compute an initial iteration budget based on graph topology, goal complexity, and experience bank.
- Stops early when any of these conditions are met:
  1. Target confidence (default 0.95) is reached.
  2. Convergence stalls — improvement over the sliding window drops below `min_improvement` (default 0.01).
  3. Hard `max_iterations` cap (default 30) is reached.
- Enforces `min_iterations` (default 2) to prevent premature stopping.

**Configuration:**
```toml
[cognitive_loop.adaptive_depth]
initial_budget = 2.0
decay = 0.3
min_improvement = 0.15
convergence_threshold = 0.05
```

### 2. ParallelBranchPredictor
Simulates multiple variant SOP graphs concurrently and selects the best branch.

**How it works:**
- Accepts a list of variant graphs (e.g., evolved variants, the original graph, heuristic alternatives).
- Runs two types of simulation in parallel for each variant:
  1. **Execution simulation** (`sisyphus_intel::Simulator`): predicts success probability, estimated cost, stability, and effort.
  2. **Logical simulation** (`LogicalSimulator`): validates logical soundness and detects flaws.
- Computes a **composite score** for each branch:
  ```
  score = 0.5 × success_probability
        + 0.3 × cost_efficiency
        + 0.2 × predicted_stability
        - logical_penalty
  ```
  Where `logical_penalty` is `0.2 × critical_flaw_count` when logical validation fails.
- Uses the `ExperienceBank` to calibrate confidence scores from historical outcomes.
- Defaults: `max_branches = 3`, `concurrency = 3`.

### 3. AnomalyDetector
Monitors per-node execution metrics at runtime and flags anomalies.

**How it works:**
- Tracks three metrics per node:
  1. **Fuel consumption** — anomalous at >2.0x baseline (default threshold).
  2. **Execution duration** — anomalous at >3.0x baseline (default), High severity at >5.0x.
  3. **Error rate** — anomalous at >30% error rate (default threshold).
- Uses **exponentially weighted moving average** (EWMA, `alpha = 0.3`) for baseline tracking, giving more weight to recent observations.
- Each `observe()` call returns inline anomalies for immediate action.
- `generate_report()` produces a global `AnomalyReport` with:
  - `should_extra_reflect` boolean — triggers extra reflection cycles in the cognitive loop.
  - `overall_severity` — Low / Medium / High / Critical.
- Integration in `StrategicManager`: anomalies from the execution step feed into the reflect step, enabling the loop to self-correct mid-execution.

## Consequences

### Positive
- **Better iteration efficiency**: adaptive depth saves 20–40% iterations on simple tasks while allowing complex tasks the iterations they need.
- **Parallel exploration**: higher probability of selecting the optimal execution path without serial trial-and-error.
- **Runtime resilience**: anomaly detection catches degradation early — fuel spikes, hung nodes, cascading failures — and triggers extra reflection before the loop escalates.
- **Historical calibration**: experience bank integration means branch selection and confidence estimates improve over time.

### Negative
- **Slightly higher memory footprint**: `AnomalyDetector` maintains per-node metric history (map from node name to EWMA state). `AdaptiveDepthController` keeps a confidence history vector.
- **Increased latency per iteration**: `ParallelBranchPredictor` runs N concurrent simulations (default 3), adding simulation overhead to the predict step.
- **Configuration surface area grows**: new `AdaptiveDepthConfig` struct with four parameters adds to the cognitive load of tuning.

### Neutral
- The fixed `max_iterations` from v1 is retained as a hard cap in `AdaptiveDepthController`, preserving backward compatibility.
- `AnomalyDetector` is optional — `StrategicManager` can be configured with a no-op detector if anomaly tracking is not needed.

## Integration Points
- `StrategicManager::run_cognitive_loop()` wires all three components: `AdaptiveDepthController` for loop termination, `AnomalyDetector` for inline monitoring, and `ParallelBranchPredictor` for branch selection.
- Source: `crates/sisyphus-core/src/cognitive_loop_v2.rs`
- Re-exported via `crates/sisyphus-core/src/lib.rs` (`pub use cognitive_loop_v2::*`).
- `CognitiveLoopConfig` in `crates/sisyphus-types/src/config/sub.rs` includes `adaptive_depth: AdaptiveDepthConfig`.
- Bridge gRPC exposes `StreamAnomalyReport` RPC for streaming anomaly data to clients.
