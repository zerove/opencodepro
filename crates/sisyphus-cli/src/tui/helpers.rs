use std::fmt::Write;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

// ── Clipboard helpers ───────────────────────────────────

/// Copy text to system clipboard.
/// First tries `arboard` (X11/Wayland), then falls back to OSC 52 terminal escape.
/// Returns Ok with a user-facing success message, or Err with a failure message.
pub fn copy_to_clipboard(text: &str) -> Result<String, String> {
    if text.is_empty() {
        return Err("Nothing to copy".into());
    }

    // Try desktop clipboard (X11/Wayland)
    if let Ok(mut cb) = arboard::Clipboard::new() {
        if cb.set_text(text).is_ok() {
            let len = text.len();
            return Ok(format!("Copied {len} bytes to clipboard"));
        }
    }

    // Fallback: OSC 52 escape sequence (works in tmux, ssh, most modern terminals)
    if let Some(seq) = build_osc52(text) {
        print!("{seq}");
        // Flush stdout so the sequence is sent immediately
        use std::io::Write as _;
        let _ = std::io::stdout().flush();
        let len = text.len();
        return Ok(format!("Copied {len} bytes (OSC 52)"));
    }

    Err("Clipboard unavailable — terminal may not support OSC 52".into())
}

/// Read text from system clipboard. Returns empty string on failure.
pub fn paste_from_clipboard() -> String {
    match arboard::Clipboard::new() {
        Ok(mut cb) => cb.get_text().unwrap_or_default(),
        Err(_) => String::new(),
    }
}

/// Build an OSC 52 escape sequence for the given text.
/// Encodes text as base64. Returns Some(seq) or None if encoding fails.
fn build_osc52(text: &str) -> Option<String> {
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    let mut seq = String::new();
    // tmux needs \ePtmux;\e...\e\\ wrapper when inside tmux
    if std::env::var("TMUX").is_ok() {
        write!(seq, "\x1bPtmux;\x1b\x1b]52;;{encoded}\x07\x1b\\").ok()?;
    } else {
        write!(seq, "\x1b]52;;{encoded}\x07").ok()?;
    }
    Some(seq)
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

/// Count how many logical lines `render_message_content` would produce for the given content.
/// Used for click-to-message mapping — must stay in sync with `render_message_content`.
pub fn count_content_lines(content: &str, _expanded: bool) -> usize {
    content.lines().count()
}

/// Split content into (thought_blocks, real_answer).
/// Both are raw text; tag lines are discarded.
pub fn split_thought_and_answer(content: &str) -> (Vec<String>, String) {
    let mut thoughts = Vec::new();
    let mut answer = String::new();
    let mut in_thought = false;
    let mut current = String::new();

    for l in content.lines() {
        let trimmed = l.trim();

        if in_thought {
            // Search for closing tag anywhere in the line
            let tag_end = trimmed.find("</thought>")
                .or_else(|| trimmed.find("</thinking"))
                .or_else(|| if trimmed == ">" { Some(0) } else { None })
                .or_else(|| trimmed.find("</"));
            if let Some(tag_start) = tag_end {
                // Everything before the tag on this line → thought content
                let before = &l[..tag_start + (l.len() - trimmed.len()).min(tag_start)];
                if !before.trim().is_empty() {
                    if !current.is_empty() {
                        current.push('\n');
                    }
                    current.push_str(before);
                }
                if !current.is_empty() {
                    thoughts.push(current.clone());
                }
                in_thought = false;
                current.clear();
                // Everything after `>` of the closing tag → answer content
                let after_tag = trimmed[tag_start..].trim_start();
                let after_gt = after_tag.bytes().position(|b| b == b'>')
                    .map(|p| after_tag[p + 1..].trim())
                    .unwrap_or("");
                if !after_gt.is_empty() {
                    if !answer.is_empty() {
                        answer.push('\n');
                    }
                    answer.push_str(after_gt);
                }
                continue;
            }
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(l);
            continue;
        }

        // Not in thought — check for opening tag
        if let Some(rest) = trimmed
            .strip_prefix("<thought>")
            .or_else(|| {
                if trimmed.starts_with("<thinking") {
                    trimmed.find('>').map(|e| trimmed[e + 1..].trim_start())
                } else {
                    None
                }
            }) {
            in_thought = true;
            current.clear();
            if rest.is_empty() {
                continue;
            }
            // Opening tag + content on same line (e.g. `<thought>inner</thought>ans`)
            let close = rest.find("</thought>")
                .or_else(|| rest.find("</thinking"))
                .or_else(|| if rest == ">" { Some(0) } else { None })
                .or_else(|| rest.find("</"));
            if let Some(pos) = close {
                let thought_part = rest[..pos].trim();
                if !thought_part.is_empty() {
                    current.push_str(thought_part);
                }
                if !current.is_empty() {
                    thoughts.push(current.clone());
                }
                in_thought = false;
                current.clear();
                let after_tag = rest[pos..].trim_start();
                if let Some(gt) = after_tag.bytes().position(|b| b == b'>') {
                    let ans = after_tag[gt + 1..].trim();
                    if !ans.is_empty() {
                        if !answer.is_empty() {
                            answer.push('\n');
                        }
                        answer.push_str(ans);
                    }
                }
            } else {
                // No closing tag — everything after `<thought>` is thought content
                current.push_str(rest);
            }
            continue;
        }

        if !answer.is_empty() {
            answer.push('\n');
        }
        answer.push_str(l);
    }

    (thoughts, answer)
}

/// Count lines for the new thought-section rendering, including the collapsible header.
/// Mirrors `render_message_with_thought_section`.
pub fn count_message_lines(content: &str, thought_expanded: bool) -> usize {
    let (thoughts, answer) = split_thought_and_answer(content);

    if thoughts.is_empty() {
        return count_content_lines(content, false);
    }

    let mut count = 0usize;

    // ── thought section ──
    count += 1; // header
    if thought_expanded {
        for t in &thoughts {
            count += t.lines().count();
        }
        if !answer.trim().is_empty() {
            count += 1; // blank separator after thought section
        }
    }

    // ── answer section ──
    count += count_content_lines(&answer, false);

    count
}

/// Render message content (non-thought lines only).
/// Parses inline markers (**bold**, *italic*, `code`) and diff +/- coloring.
pub fn render_message_content(lines: &mut Vec<Line<'static>>, content: &str, _expanded: bool) {
    for l in content.lines() {
        // Diff coloring: lines starting with +/- get green/red
        if l.starts_with("+ ") || l == "+" {
            lines.push(Line::from(Span::styled(
                l.to_string(),
                Style::default().fg(Color::Rgb(0x7f, 0xd8, 0x8f)),
            )));
            continue;
        }
        if l.starts_with("- ") || l == "-" {
            lines.push(Line::from(Span::styled(
                l.to_string(),
                Style::default().fg(Color::Rgb(0xe0, 0x6c, 0x75)),
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
}

fn indent_spans(prefix: &str, spans: &[Span<'static>]) -> Vec<Span<'static>> {
    let mut out = vec![Span::raw(prefix.to_string())];
    out.extend(spans.iter().cloned());
    out
}

/// Render a message with thought blocks in a separate collapsible section.
/// Thought blocks get a clickable/tappable header line; the real answer always shows.
/// For messages without thought blocks, falls back to standard rendering.
pub fn render_message_with_thought_section(
    lines: &mut Vec<Line<'static>>,
    content: &str,
    thought_expanded: bool,
) {
    let (thoughts, answer) = split_thought_and_answer(content);

    if thoughts.is_empty() {
        render_message_content(lines, content, false);
        return;
    }

    let total_lines: usize = thoughts.iter().map(|t| t.lines().count()).sum();
    let clickable = Color::Rgb(0xcc, 0x99, 0x44);  // amber — indicates interactive
    let thought_fg = Color::Rgb(0x9a, 0x9a, 0x9a);
    let clickable_style = Style::default().fg(clickable).add_modifier(Modifier::BOLD);

    // ── Thought header (clickable) ─────────────────────────────────
    let header = if thought_expanded {
        "▼ 思考过程 [收起] ".to_string()
    } else if total_lines > 0 {
        format!("▶ 思考过程 ({}行) [展开] ", total_lines)
    } else {
        "▶ 思考过程 [展开] ".into()
    };
    lines.push(Line::from(vec![
        Span::styled(header, clickable_style),
        Span::styled("← click", Style::default().fg(thought_fg)),
    ]));

    // ── Thought content (expanded only) ────────────────────────────
    if thought_expanded {
        for thought in &thoughts {
            for l in thought.lines() {
                let styled = parse_inline_style(l);
                let dimmed: Vec<Span> = if styled.is_empty() {
                    vec![Span::raw("")]
                } else {
                    styled
                        .into_iter()
                        .map(|s| {
                            Span::styled(
                                format!("  {}", s.content),
                                Style::default().fg(Color::Rgb(0x7a, 0x7a, 0x7a)),
                            )
                        })
                        .collect()
                };
                if dimmed.len() == 1 {
                    lines.push(Line::from(dimmed));
                } else {
                    lines.push(Line::from(indent_spans("  ", &dimmed)));
                }
            }
        }

        // ── Separator line before answer ───────────────────────────
        if !answer.trim().is_empty() {
            lines.push(Line::from(Span::styled(
                "─── 回答 ───",
                Style::default().fg(Color::Rgb(0x55, 0x55, 0x55)),
            )));
        }
    }

    // ── Answer section ── (always visible)
    render_message_content(lines, &answer, false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_thought_and_answer_basic() {
        let content = "hello world";
        let (thoughts, answer) = split_thought_and_answer(content);
        assert!(thoughts.is_empty());
        assert_eq!(answer, "hello world");
    }

    #[test]
    fn test_split_thought_and_answer_closing_on_same_line() {
        let content = concat!(
            "<thought>internal reasoning\n",
            "more thinking</thought>This is the final answer\n",
            "second line"
        );
        let (thoughts, answer) = split_thought_and_answer(content);
        assert_eq!(thoughts.len(), 1);
        assert!(thoughts[0].contains("internal reasoning"));
        assert!(thoughts[0].contains("more thinking"));
        assert_eq!(answer, "This is the final answer\nsecond line");
    }

    #[test]
    fn test_split_thought_and_answer_no_thought() {
        let content = "Just an answer\nwith multiple lines";
        let (thoughts, answer) = split_thought_and_answer(content);
        assert!(thoughts.is_empty());
        assert_eq!(answer, content);
    }

    #[test]
    fn test_split_thought_and_answer_empty_thought() {
        let content = "<thought></thought>answer here";
        let (thoughts, answer) = split_thought_and_answer(content);
        assert!(thoughts.is_empty());
        assert_eq!(answer, "answer here");
    }
}


