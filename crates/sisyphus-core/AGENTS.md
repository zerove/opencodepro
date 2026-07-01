# Sisyphus Core Agent Knowledge Base

## OVERVIEW
Core cognitive engine: DAG execution, cognitive loop, SOP orchestration.

## STRUCTURE
- `src/lib.rs`: Crate root — `CoreError`, `Consensus`, `SisyphusKernel` struct + public API
- `src/dag/`: DAG execution engine (`DependencyResolver`, `build_dep_ctx_for`, tests)
- `src/executor.rs`: Task execution with async dispatch (`Executor`, `execute_tasks_in_parallel`)
- `src/cognitive_loop.rs`: predict → execute → reflect → evolve cycle (`execute_with_cognition`)
- `src/intent_gate.rs`: Goal → execution mode classification (`IntentGate`)
- `src/budget.rs`: `BudgetProvider` impl for SisyphusKernel (cost check / record)
- `src/tests.rs`: Lib-level tests (kernel, executor, error types)

## WHERE TO LOOK
- Cognitive loop entry: `cognitive_loop.rs` (`execute_with_cognition`)
- DAG scheduling: `dag/` module (topological resolution, composite scheduling)
- Goal execution pipeline: `lib.rs` (`execute_goal`, `execute_goals`, `execute_and_record`)
- Strategic planning: `lib.rs` (`strategic_reflect`, `generate_milestones`, `project_costs`)
- Composite execution: `lib.rs` (`execute_composite`, uses `dag::DependencyResolver`)
- Integration tests: `tests/` (cognitive_loop_test, composite_test, executor_test, etc.)

## CONVENTIONS
- All async functions use `tokio` + `async-trait`
- Errors via `CoreError` (thiserror), never `unwrap()`/`expect()` in production
- Cognitive loop always follows predict → execute → reflect → evolve decomposition

## COMMANDS
```bash
cargo test -p sisyphus-core         # Full test suite (wasmtime-heavy)
cargo test -p sisyphus-core --lib   # Lib-only tests (~2s)
cargo bench -p sisyphus-core        # Cognitive loop benchmarks
make test-core                      # Full suite with CARGO_BUILD_JOBS=1
make test-core-quick                # Lib-only quick test
```
