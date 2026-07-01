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
- Maintain `From<TypesError> for String` for legacy compatibility.

## COMMANDS
```bash
cargo test -p sisyphus-types
```
