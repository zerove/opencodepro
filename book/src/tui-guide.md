# TUI Guide

Sisyphus includes a full-featured Terminal User Interface built with ratatui.

## Navigation

| Key | Action |
|-----|--------|
| `Ctrl+K` | Open command palette |
| `Ctrl+L` | Session list |
| `Ctrl+M` | Model list |
| `Ctrl+A` | Agent list |
| `Ctrl+N` | New session |
| `Ctrl+H` | Go home |
| `Ctrl+Q` | Exit |
| `F1` | Help |

## Input Editing

| Key | Action |
|-----|--------|
| `Home` / `End` | Move to start/end of line |
| `Ctrl+U` | Delete to start of line |
| `Ctrl+W` | Delete word backward |
| `Ctrl+V` | Paste from clipboard |

## Slash Commands

Type `/` at the start of input to trigger the slash command completion popup.

Available commands:

| Command | Description |
|---------|-------------|
| `/help` | Show keybindings & help |
| `/exit` | Exit Sisyphus |
| `/clear` | Clear the log |
| `/theme` | Toggle dark/light theme |
| `/sessions` | Switch or manage sessions |
| `/new` | Create a new session |
| `/model` | Switch AI model |
| `/agent` | Switch agent |
| `/dev` | Run autonomous dev agent |
| `/todos` | Manage todo list |
| `/lsp` | View LSP diagnostics |
| `/sop` | List & execute SOPs |
| `/skills` | Browse & install skills |
| `/roadmap` | View project roadmap |
| `/bridge` | Check bridge health |
| `/home` | Return to Home screen |
| `/search` | Search messages in session |

### Navigation

- `Up` / `Down` - Navigate the completion list
- `Tab` / `Enter` - Accept the selected completion
- `Esc` - Close the completion popup

## @file References

Reference files in your input with `@` prefix:

```
Look at @src/main.rs and @lib.rs
```

The file contents will be automatically expanded with a line limit of 200 lines.

## Session Features

- **Fork**: Create a branch from any session
- **Undo**: Revert the last message
- **Rename**: Change session name
- **Compress**: Summarize old messages to save tokens
- **Checkpoint**: Save/restore session state
- **Export**: Export as JSON or HTML
- **Import**: Import from JSON

## Search

Press `/search` in a session to activate search mode. Navigate matches with `n` (next) and `p` (previous).
