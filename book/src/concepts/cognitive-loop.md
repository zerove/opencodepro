# Cognitive Loop

The recursive **OODA-inspired** cycle that drives Sisyphus's self-improvement behavior.

## Phases

```
1. SYNTHESIZE ─────────────────────────────┐
   │ Goal → SOP Graph                       │
   ▼                                        │
2. EXECUTE ──┐                              │
   │ SOP →    │ (loop back                  │
   │ Results  │  on evolve)                 │
   ▼          │                              │
3. SIMULATE   │                              │
   │ Traces → │                              │
   │ Predictions                             │
   ▼          │                              │
4. REFLECT    │                              │
   │ Actual vs│                              │
   │ Predicted│                              │
   ▼          │                              │
5. DECIDE     │                              │
   │ Converge?│                              │
   │          │                              │
   ├─ Yes ────┘                              │
   │   Return result                         │
   │                                          │
   └─ No ────────────────────────────────────┘
        Evolve SOP → loop to EXECUTE
```

## Convergence

The loop terminates when one of these conditions is met:

- **Deviation threshold**: The reflection deviation score drops below `CONVERGENCE_THRESHOLD` (typically 0.15).
- **Max iterations**: The loop has executed `MAX_COGNITIVE_ITERATIONS` (default 10) times without converging.
- **No evolution**: The evolver returns `None`, indicating no further improvements are possible.

## Metrics

Each iteration records:
- Iteration number and deviation score
- Whether it converged
- Duration of the iteration
- Evolution actions taken (fuel changes, retry wrapper, etc.)
