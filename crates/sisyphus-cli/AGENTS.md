# Sisyphus CLI Agent Knowledge Base

## OVERVIEW
TUI/CLI client for the Sisyphus cognitive engine (ratatui + crossterm).

## STRUCTURE
- `src/main.rs`: Entry point, argument parsing, TUI launch
- `src/bridge.rs`: gRPC bridge client (connect, health check, add_message)
- `src/tui/mod.rs`: App state, event loop, view routing, mouse/scroll handlers
- `src/tui/helpers.rs`: Rendering helpers (thought blocks, clipboard, style parsing)
- `src/tui/background.rs`: Background bridge agent, LLM response polling
- `src/tui/sessions.rs`: Session list rendering and management

## WHERE TO LOOK
- Thought block rendering: `helpers.rs` (`split_thought_and_answer`, `render_message_with_thought_section`)
- Bridge interaction: `bridge.rs` (connect with timeout, IPv6→IPv4 fallback)
- Background polling: `background.rs` (add_message timeout config)
- Click/scroll handling: `mod.rs` (mouse event dispatch)
- Clipboard: `helpers.rs` (OSC 52 fallback)

## CONVENTIONS
- Bridge address from `Config.bridge_address`; `--bridge` CLI flag overrides
- Thought blocks parsed from `<thought>` / `</thought>` tags in LLM responses
- Permissive click toggle (anywhere on message toggles thought block)

## COMMANDS
```bash
cargo build -p sisyphus-cli        # Build (debug)
cargo build --release -p sisyphus-cli  # Release build
cargo test -p sisyphus-cli         # Run tests
make run-tui                       # Quick launch
make dev-tui                       # Auto-rebuild on file change
```
