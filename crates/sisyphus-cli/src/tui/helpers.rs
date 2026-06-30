use ratatui::{
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
};

use super::App;

// ── Clipboard helpers ───────────────────────────────────

/// Copy text to system clipboard. Logs failure silently.
pub fn copy_to_clipboard(text: &str) {
    if text.is_empty() {
        return;
    }
    match arboard::Clipboard::new() {
        Ok(mut cb) => {
            if let Err(e) = cb.set_text(text) {
                eprintln!("[clipboard] copy failed: {e}");
            }
        }
        Err(e) => {
            eprintln!("[clipboard] init failed: {e}");
        }
    }
}

/// Read text from system clipboard. Returns empty string on failure.
pub fn paste_from_clipboard() -> String {
    match arboard::Clipboard::new() {
        Ok(mut cb) => cb.get_text().unwrap_or_default(),
        Err(_) => String::new(),
    }
}

// ── Scroll indicator ────────────────────────────────────

pub fn scroll_pct(pos: usize, total: usize) -> String {
    if total <= 1 {
        String::new()
    } else {
        let pct = (pos as f64 / (total.saturating_sub(1) as f64) * 100.0).round() as u8;
        format!(" {pos}/{total} ({pct}%) ")
    }
}

// ── Message content rendering (shared by sessions + todos) ──

/// Parse inline style markers in a line of text, returning styled spans.
/// Supports: `**bold**`, `*italic*`, `` `code` ``.
pub fn parse_inline_style(line: &str) -> Vec<Span<'static>> {
    use ratatui::style::Modifier;
    let mut spans = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // bold (**)
        if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
            let mut end = i + 2;
            while end + 1 < chars.len() && !(chars[end] == '*' && chars[end + 1] == '*') {
                end += 1;
            }
            if end + 1 < chars.len() {
                let content: String = chars[i + 2..end].iter().collect();
                spans.push(Span::styled(
                    content,
                    Style::default().add_modifier(Modifier::BOLD),
                ));
                i = end + 2;
                continue;
            }
        }
        // italic (*)
        if chars[i] == '*' {
            let mut end = i + 1;
            while end < chars.len() && chars[end] != '*' {
                end += 1;
            }
            if end < chars.len() {
                let content: String = chars[i + 1..end].iter().collect();
                spans.push(Span::styled(
                    content,
                    Style::default().add_modifier(Modifier::ITALIC),
                ));
                i = end + 1;
                continue;
            }
        }
        // code (`)
        if chars[i] == '`' {
            let mut end = i + 1;
            while end < chars.len() && chars[end] != '`' {
                end += 1;
            }
            if end < chars.len() {
                let content: String = chars[i + 1..end].iter().collect();
                    spans.push(Span::styled(content, Style::default().fg(Color::Rgb(0x56, 0xb6, 0xc2))));
                i = end + 1;
                continue;
            }
        }
        // line break handling
        if chars[i] == '\n' {
            spans.push(Span::raw("\n"));
            i += 1;
            continue;
        }
        // regular char
        let mut end = i + 1;
        while end < chars.len()
            && chars[end] != '*'
            && chars[end] != '`'
            && chars[end] != '\n'
        {
            end += 1;
        }
        let content: String = chars[i..end].iter().collect();
        spans.push(Span::raw(content));
        i = end;
    }
    spans
}

/// Render message content, collapsing `<thought>` / `<thinking` blocks into a single summary line.
pub fn render_message_content(lines: &mut Vec<Line<'static>>, content: &str) {
    let mut in_thought = false;
    let mut thought_lines = 0usize;

    for l in content.lines() {
        let trimmed = l.trim();

        if trimmed.starts_with("<thought>") || trimmed.starts_with("<thinking") {
            in_thought = true;
            thought_lines = 1;
            continue;
        }

        if in_thought {
            if trimmed.starts_with("</thought>") || trimmed.starts_with("</thinking")
                || trimmed == ">" || trimmed.starts_with("</")
            {
                in_thought = false;
                let label = if thought_lines > 1 {
                    format!(" [thought block: {thought_lines} lines collapsed] ")
                } else {
                    " [1 thought line collapsed] ".to_string()
                };
                lines.push(Line::from(Span::styled(
                    label,
                    Style::default().fg(Color::Rgb(0x6a, 0x6a, 0x6a)).dim().add_modifier(Modifier::DIM),
                )));
                continue;
            }
            thought_lines += 1;
            continue;
        }

        // Diff coloring: lines starting with +/- get green/red
        if l.starts_with("+ ") || l == "+" {
            lines.push(Line::from(Span::styled(
                l.to_string(),
                Style::default().fg(Color::Rgb(0x7f, 0xd8, 0x8f)), // theme.success green
            )));
            continue;
        }
        if l.starts_with("- ") || l == "-" {
            lines.push(Line::from(Span::styled(
                l.to_string(),
                Style::default().fg(Color::Rgb(0xe0, 0x6c, 0x75)), // theme.error red
            )));
            continue;
        }

        let styled = parse_inline_style(l);
        if styled.is_empty() {
            lines.push(Line::from(""));
        } else {
            lines.push(Line::from(styled));
        }
    }

    // If thought block was never closed, still report it
    if in_thought && thought_lines > 0 {
        let label = if thought_lines > 1 {
            format!(" [thought block: {thought_lines} lines, unclosed] ")
        } else {
            " [1 unclosed thought line] ".to_string()
        };
        lines.push(Line::from(Span::styled(
            label,
            Style::default().fg(Color::DarkGray).dim().add_modifier(Modifier::DIM),
        )));
    }
}

// ── Tab hit-testing ─────────────────────────────────────

pub fn tab_at_x(_app: &App, x: u16) -> Option<super::Panel> {
    let mut cur_x: u16 = 0;
    for panel in super::Panel::all() {
        let label = panel.name();
        let tab_width = label.len() as u16 + 4; // " P " with spacing
        if x >= cur_x && x < cur_x + tab_width {
            return Some(*panel);
        }
        cur_x += tab_width;
    }
    None
}
