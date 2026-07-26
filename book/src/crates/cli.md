# sisyphus-cli — Command-Line Interface

The CLI binary provides direct access to Sisyphus tools and agents from the terminal. It supports both one-shot commands and an interactive TUI mode.

## Installation

```bash
cargo build --release -p sisyphus-cli
./target/release/sisyphus-cli --help
```

## Command Structure

```
sisyphus [OPTIONS] <COMMAND>
```

### Global Options

| Flag | Env | Description |
|------|-----|-------------|
| `-i`, `--interactive` | | Launch interactive TUI mode |
| `-b`, `--bridge` | `SISYPHUS_BRIDGE` | Bridge gRPC address (default: `http://[::1]:50051`) |
| `--data-dir` | `SISYPHUS_DATA_DIR` | Data directory for todo/session storage |
| `--config` | `SISYPHUS_CONFIG` | Path to TOML config file |

## File Tools

```bash
# Read a file
sisyphus read path/to/file.txt

# Write content to a file
sisyphus write path/to/output.txt "file content"

# Execute a shell command
sisyphus exec "cargo test" --dir /path/to/project

# Search the web
sisyphus search "Rust async/await" -n 10
```

## Agent Subcommands

### librarian — External Research Agent

Search the web, fetch documentation, and find code examples.

```bash
# Research a topic (web, docs, code, or all)
sisyphus librarian research "Rust async" --scope all -n 5

# Fetch and display documentation from a URL
sisyphus librarian fetch-docs https://doc.rust-lang.org/std/fmt/

# Find code examples for a library or API
sisyphus librarian examples "serde_json::from_str" -l rust

# Synthesise findings from a JSON research report
sisyphus librarian synthesise findings.json
```

`librarian research` supports the following scopes via `--scope`:
- `web` — Search the web for general information
- `docs` — Fetch documentation pages
- `code` — Find code examples
- `all` — All of the above (default)

### metis — Pre-Planning Analysis Agent

Analyse a request for ambiguities and implicit requirements before implementation.

```bash
# Analyse inline request text
sisyphus metis analyse "Add user authentication with OAuth2 support"

# Analyse request from a file
sisyphus metis analyse --file request.txt
```

The analysis output includes:
- **Summary** — High-level assessment of the request
- **Ambiguities** — Aspects that need clarification, with severity levels and suggested resolutions
- **Implicit Requirements** — Requirements that can be inferred but aren't explicitly stated

### momus — Plan/Code/Architecture Review Agent

Review plans, source code, and architecture for completeness and consistency.

```bash
# Review a plan file
sisyphus momus review-plan plan.md

# Review a source code file
sisyphus momus review-code src/main.rs

# Review architecture documentation inline
sisyphus momus review-arch "The system uses microservices with message queues"

# Review all Rust files in a directory
sisyphus momus review-dir src/
```

## Task & Session Management

```bash
# Add a todo item
sisyphus todo add "Fix login bug" -p high

# List pending todos
sisyphus todo list --status pending

# Create a new session
sisyphus session create "Refactoring session"

# List available sessions
sisyphus session list
```

## Bridge Commands

```bash
# Execute a Standard Operating Procedure via the bridge gRPC
sisyphus sop run goal:deploy_to_staging

# List available SOPs
sisyphus sop list

# Check bridge health
sisyphus health
```

## Workspace Management

Workspaces provide isolated project environments with independent state (SOPs, sessions, todos).

```bash
# Create a new workspace
sisyphus workspace create "My Project" --project-id proj_alpha --root-dir /path/to/project

# List all workspaces
sisyphus workspace list

# Switch to a workspace
sisyphus workspace switch <workspace-id>

# Show current workspace
sisyphus workspace current

# Delete a workspace (with confirmation prompt)
sisyphus workspace delete <workspace-id>

# Delete a workspace (skip confirmation)
sisyphus workspace delete <workspace-id> -y
```

All data persisted within a workspace (SOPs, sessions, todos, experiences) is isolated by `project_id`. Switching workspaces changes the active project context so subsequent commands operate in that project's storage namespace.

## Interactive TUI

Launch the interactive terminal UI:

```bash
sisyphus --interactive
```
