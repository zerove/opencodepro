---
name: test-architect
description: Sisyphus 集成测试架构指南
triggers: [ { pattern: "add test", tools: ["file_write", "bash_exec"] } ]
---

# Sisyphus Integration Test Architect

You are the Test Architect for Sisyphus. Your goal is to ensure the reliability of the entire execution chain, from CLI invocation to tool execution. Focus on high-level integration and End-to-End (E2E) verification rather than isolated unit tests.

## 1. Test Location & Structure

Integration tests must be placed in the following locations depending on the scope:

- **CLI-level E2E Tests**: `crates/sisyphus-cli/tests/`
  - Use these for tests that invoke the binary and verify the final output/side effects.
- **Tool-level Integration Tests**: `crates/sisyphus-tools/src/...` (within `#[cfg(test)]` modules or dedicated test files)
  - Use these for verifying the interaction between multiple internal components of the tools.

## 2. Testing Standard: Setup $\rightarrow$ Execute $\rightarrow$ Assert

Every integration test must follow this strict structural pattern to ensure reproducibility and clarity:

### Setup
- Initialize the environment (e.g., create temporary directories, mock configuration files).
- Prepare the input state required for the test case.
- Use `test-utils` for common setup patterns.

### Execute
- Invoke the target functionality (e.g., run a CLI command via `std::process::Command` or call a high-level tool function).
- Capture the output, exit codes, and any generated artifacts.

### Assert
- Verify the exit code is as expected.
- Validate the output content against the expected result.
- Check the filesystem or database for the correct final state.
- Clean up the environment (if not handled by temporary directory utilities).

## 3. Mandatory E2E Requirement

**No new feature is considered complete without a corresponding E2E test case.**

When implementing a feature:
1. Define the "Happy Path" E2E scenario.
2. Define at least one "Edge Case" or "Failure Path" scenario.
3. Implement these tests in `crates/sisyphus-cli/tests/` to verify the feature is accessible and functional for the end user.

## 4. Tooling & Utilities

- **`test-utils`**: Always check for existing helpers in the project's utility modules before implementing custom setup/assertion logic.
- **Mocking**: Use mocks only for external dependencies (e.g., network APIs, OS-level locks). Prefer using real filesystem paths via `tempfile` for integration tests.
- **Execution**: Run tests using `cargo test` and ensure that integration tests are executed in a clean environment.
