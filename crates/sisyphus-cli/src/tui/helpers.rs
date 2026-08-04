use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use std::fmt::Write;

// ── 剪贴板辅助 ───────────────────────────────────

/// 复制文本到系统剪贴板。
/// 先尝试 `arboard`（X11/Wayland），失败则回退到 OSC 52 终端转义。
/// 成功时返回带用户提示信息的 Ok，失败时返回错误消息的 Err。
pub fn copy_to_clipboard(text: &str) -> Result<String, String> {
    if text.is_empty() {
        return Err("Nothing to copy".into());
    }

    // 尝试桌面剪贴板（X11/Wayland）
    if let Ok(mut cb) = arboard::Clipboard::new() {
        if cb.set_text(text).is_ok() {
            let len = text.len();
            return Ok(format!("Copied {len} bytes to clipboard"));
        }
    }

    // 回退：OSC 52 转义序列（适用于 tmux、ssh 及大多数现代终端）
    if let Some(seq) = build_osc52(text) {
        print!("{seq}");
        // 刷新 stdout 使序列立即发送
        use std::io::Write as _;
        let _ = std::io::stdout().flush();
        let len = text.len();
        return Ok(format!("Copied {len} bytes (OSC 52)"));
    }

    Err("Clipboard unavailable — terminal may not support OSC 52".into())
}

/// 从系统剪贴板读取文本。失败时返回空字符串。
pub fn paste_from_clipboard() -> String {
    match arboard::Clipboard::new() {
        Ok(mut cb) => cb.get_text().unwrap_or_default(),
        Err(_) => String::new(),
    }
}

/// 为给定文本构建 OSC 52 转义序列。
/// 将文本编码为 base64。成功返回 Some(seq)，编码失败返回 None。
fn build_osc52(text: &str) -> Option<String> {
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    let mut seq = String::new();
    // 在 tmux 内部时需要 \ePtmux;\e...\e\\ 包装
    if std::env::var("TMUX").is_ok() {
        write!(seq, "\x1bPtmux;\x1b\x1b]52;;{encoded}\x07\x1b\\").ok()?;
    } else {
        write!(seq, "\x1b]52;;{encoded}\x07").ok()?;
    }
    Some(seq)
}

// ── 滚动指示器 ────────────────────────────────────

pub fn scroll_pct(pos: usize, total: usize) -> String {
    if total <= 1 {
        String::new()
    } else {
        let pct = (pos as f64 / (total.saturating_sub(1) as f64) * 100.0).round() as u8;
        format!(" {pos}/{total} ({pct}%) ")
    }
}

// ── 消息内容渲染（会话与待办共用） ──

/// 解析一行文本中的内联样式标记，返回带样式的片段。
/// 支持：`**bold**`、`*italic*`、`` `code` ``。
pub fn parse_inline_style(line: &str) -> Vec<Span<'static>> {
    use ratatui::style::Modifier;
    let mut spans = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // 粗体（**）
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
        // 斜体（*）
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
        // 代码（`）
        if chars[i] == '`' {
            let mut end = i + 1;
            while end < chars.len() && chars[end] != '`' {
                end += 1;
            }
            if end < chars.len() {
                let content: String = chars[i + 1..end].iter().collect();
                spans.push(Span::styled(
                    content,
                    Style::default().fg(Color::Rgb(0x56, 0xb6, 0xc2)),
                ));
                i = end + 1;
                continue;
            }
        }
        // 换行处理
        if chars[i] == '\n' {
            spans.push(Span::raw("\n"));
            i += 1;
            continue;
        }
        // 普通字符
        let mut end = i + 1;
        while end < chars.len() && chars[end] != '*' && chars[end] != '`' && chars[end] != '\n' {
            end += 1;
        }
        let content: String = chars[i..end].iter().collect();
        spans.push(Span::raw(content));
        i = end;
    }
    spans
}

/// 计算 `render_message_content` 对给定内容会生成多少逻辑行。
/// 用于点击定位消息 — 必须与 `render_message_content` 保持同步。
pub fn count_content_lines(content: &str, _expanded: bool) -> usize {
    content.lines().count()
}

/// 将内容拆分为 (thought_blocks, real_answer)。
/// 两者都是原始文本；标签行被丢弃。
pub fn split_thought_and_answer(content: &str) -> (Vec<String>, String) {
    let mut thoughts = Vec::new();
    let mut answer = String::new();
    let mut in_thought = false;
    let mut current = String::new();

    for l in content.lines() {
        let trimmed = l.trim();

        if in_thought {
            // 在行内任意位置查找闭合标签
            let tag_end = trimmed
                .find("</thought>")
                .or_else(|| trimmed.find("</thinking"))
                .or_else(|| if trimmed == ">" { Some(0) } else { None })
                .or_else(|| trimmed.find("</"));
            if let Some(tag_start) = tag_end {
                // 该行标签之前的所有内容 → 思考内容
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
                // 闭合标签 `>` 之后的所有内容 → 回答内容
                let after_tag = trimmed[tag_start..].trim_start();
                let after_gt = after_tag
                    .bytes()
                    .position(|b| b == b'>')
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

        // 不在思考中 — 检查开始标签
        if let Some(rest) = trimmed.strip_prefix("<thought>").or_else(|| {
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
            // 开始标签与内容在同一行（例如 `<thought>inner</thought>ans`）
            let close = rest
                .find("</thought>")
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
                // 无闭合标签 — `<thought>` 之后的所有内容都是思考内容
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

/// 为新思考区渲染计算行数，含可折叠标题。
/// 与 `render_message_with_thought_section` 保持一致。
pub fn count_message_lines(content: &str, thought_expanded: bool) -> usize {
    let (thoughts, answer) = split_thought_and_answer(content);

    if thoughts.is_empty() {
        return count_content_lines(content, false);
    }

    let mut count = 0usize;

    // ── 思考区 ──
    count += 1; // header
    if thought_expanded {
        for t in &thoughts {
            count += t.lines().count();
        }
        if !answer.trim().is_empty() {
            count += 1; // blank separator after thought section
        }
    }

    // ── 回答区 ──
    count += count_content_lines(&answer, false);

    count
}

/// 渲染消息内容（仅非思考行）。
/// 解析内联标记（**bold**、*italic*、`code`）与 diff +/- 着色。
pub fn render_message_content(lines: &mut Vec<Line<'static>>, content: &str, _expanded: bool) {
    for l in content.lines() {
        // diff 着色：以 +/- 开头的行显示绿色/红色
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

/// 在独立可折叠区域中渲染带思考块的消息。
/// 思考块有可点击的标题行；真实回答始终显示。
/// 无思考块的消息回退到标准渲染。
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
    let clickable = Color::Rgb(0xcc, 0x99, 0x44); // amber — indicates interactive
    let thought_fg = Color::Rgb(0x9a, 0x9a, 0x9a);
    let clickable_style = Style::default().fg(clickable).add_modifier(Modifier::BOLD);

    // ── 思考区标题（可点击） ─────────────────────────────────
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

    // ── 思考区内容（仅展开时） ────────────────────────────
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

        // ── 回答前的分隔线 ───────────────────────────
        if !answer.trim().is_empty() {
            lines.push(Line::from(Span::styled(
                "─── 回答 ───",
                Style::default().fg(Color::Rgb(0x55, 0x55, 0x55)),
            )));
        }
    }

    // ── 回答区 ──（始终可见）
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
