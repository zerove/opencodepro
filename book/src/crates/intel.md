# sisyphus-intel — Cognition & Simulation

The "brain" of Sisyphus — four core cognitive capabilities: **flow synthesis**, **simulation**, **reflection**, and **evolution**.

## Four Cognitive Traits

### 1. FlowSynthesizer (Synthesis)

Converts a natural-language goal into a DAG of typed actions:

```rust
#[async_trait]
pub trait FlowSynthesizer: Send + Sync {
    async fn synthesize(&self, goal: &str, context: &ExecutionContext)
        -> Result<SynthesisResult, String>;
}
```

**Impl: `GoalDrivenSynthesizer`** — keyword-based parser that maps action verbs (fetch, process, store, search, validate) to linear or parallel DAG pipelines.

### 2. Simulator (Prediction)

Estimates success probability and execution cost before running:

```rust
#[async_trait]
pub trait Simulator: Send + Sync {
    async fn simulate(&self, graph: &SOPGraph, ctx: &ExecutionContext, sop_id: &str)
        -> Result<SimulationResult, String>;
}
```

**Impl: `MonteCarloSimulator`** — random path sampling through the DAG. Computes blended path probabilities and estimated costs. Uses execution history from the state store to bias sampling toward historically successful paths.

### 3. Reflector (Analysis)

Compares predicted vs actual outcomes:

```rust
#[async_trait]
pub trait Reflector: Send + Sync {
    async fn reflect(&self, trace: &str, prediction: &SimulationResult)
        -> Result<ReflectionReport, String>;
}
```

**Impl: `StandardReflector`** — string-based error classification building a deviation score (0.0 = perfect match). Generates structured improvement suggestions.

### 4. SOPEvolver (Adaptation)

Mutates the SOP after reflection:

```rust
#[async_trait]
pub trait SOPEvolver: Send + Sync {
    fn evolve(&self, graph: &SOPGraph, report: &ReflectionReport, prediction: &SimulationResult)
        -> Result<SOPGraph, String>;
}
```

**Impl: `StandardEvolver`** — wraps failed nodes in retry logic, adjusts fuel budgets. Uses either `SimpleFuelPricer` (linear) or `AdaptiveFuelPricer` (exponential backoff).

## Error Classification

`ErrorCategory` classifies execution failures:

```rust
pub enum ErrorCategory {
    ResourceExhaustion(&'static str),
    Timeout,
    DataError,
    Unknown,
}
```

Used by `StandardReflector` to determine severity and generate targeted improvement suggestions.

## Example

```rust
use sisyphus_intel::*;

let executor = Arc::new(MockExecutor);
let synthesizer = GoalDrivenSynthesizer::new(executor);
let ctx = ExecutionContext::new();
let result = synthesizer.synthesize("fetch process store", &ctx).await?;
println!("Graph has {} nodes", result.graph.graph.node_count());
```
