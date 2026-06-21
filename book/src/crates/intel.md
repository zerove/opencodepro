# sisyphus-intel — Cognition & Simulation

Provides the cognitive capabilities: Monte Carlo simulation, structured reflection, and SOP evolution.

## Key Components

### `SisyphusIntel`

Holds the synthesizer and reflector. Orchestrates goal synthesis and the cognitive loop's reasoning steps.

### `StandardReflector`

Three core responsibilities:

1. **Simulation** (`simulate_paths`): Runs Monte Carlo over the SOP graph — samples execution paths from source to sink nodes, computing path probabilities and blended costs.

2. **Reflection** (`reflect`): Compares simulated predictions against actual execution traces. Computes a *deviation score* (0.0 = perfect match) and generates structured improvement suggestions.

3. **Evolution** (`evolve`): Mutates the SOP graph based on reflection output — adds retry wrappers around failed nodes, adjusts fuel budgets via `AdaptiveFuelPricer`, and restructures subgraphs as needed.

### `AdaptiveFuelPricer`

Exponential backoff fuel pricer that increases fuel for nodes that ran out, and decreases it for nodes that consistently succeed.

### `MonteCarloSimulator`

Core simulation engine using random path sampling through the DAG to estimate:
- Success probability per path
- Expected execution cost
- Best path identification
