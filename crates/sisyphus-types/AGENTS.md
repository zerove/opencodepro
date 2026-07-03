# Sisyphus Types Agent Knowledge Base

## OVERVIEW
Foundation types and configuration for the Sisyphus workspace.

## WHERE TO LOOK
- **Config**: `src/config.rs` (Config struct, `apply_env_overrides`)
- **Errors**: `src/errors.rs` (TypesError enum)
- **Domain Types**: `src/types.rs`, `src/execution.rs`, `src/graph.rs`, `src/node.rs`, `src/roadmap.rs`

## CONVENTIONS
- Config changes must be synchronized with `config.example.toml` and `book/src/configuration.md`.
- Use `thiserror` for all new `TypesError` variants.
- All error-to-String conversions must use `.to_string()` (from `Display` trait) explicitly.
  Never add `From<ErrorType> for String` impls.

## COMMANDS
```bash
cargo test -p sisyphus-types
```
