// ── 共享 TUI 组件辅助 ─────────────────────────────
//
// 整合常见渲染模式以减少重复，
// 覆盖对话框、会话与渲染模块。

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, Borders, List, ListItem},
};

use crate::tui::theme::Theme;

// ── 滚动指示器 ───────────────────────────────────

/// 将滚动位置格式化为 "pos/total (pct%)"。
/// 当 total <= 1（无需滚动）时返回空字符串。
pub fn scroll_pct(pos: usize, total: usize) -> String {
    if total <= 1 {
        String::new()
    } else {
        let pct = (pos as f64 / (total.saturating_sub(1) as f64) * 100.0).round() as u8;
        format!(" {pos}/{total} ({pct}%) ")
    }
}

// ── 通用列表渲染器 ──────────────────────────────

/// 在带边框的块中渲染可选中列表。
///
/// `items` 是已渲染的 `ListItem`，`selected` 是当前
/// 选中索引（钳制到 `items.len()`）。
pub fn render_list(
    frame: &mut ratatui::Frame,
    area: Rect,
    items: Vec<ListItem>,
    selected: usize,
    title: String,
    theme: &Theme,
) {
    let count = items.len();
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(theme.primary)),
        )
        .highlight_style(Style::default().fg(theme.secondary).add_modifier(Modifier::REVERSED));
    let sel = selected.min(count.saturating_sub(1));
    frame.render_stateful_widget(
        list,
        area,
        &mut ratatui::widgets::ListState::default().with_selected(Some(sel)),
    );
}

// ── 自编程状态与质量评估卡片 ─────────────────────────

/// 渲染自编程状态与质量评估摘要卡片
pub fn render_self_programming_card(
    frame: &mut ratatui::Frame,
    area: Rect,
    tool_id: &str,
    quality_score: f64,
    status_msg: &str,
    theme: &Theme,
) {
    use ratatui::{
        text::{Line, Span},
        widgets::{Paragraph, Wrap},
    };
    let text = vec![
        Line::from(vec![
            Span::styled("Tool ID: ", Style::default().fg(theme.primary)),
            Span::raw(tool_id),
        ]),
        Line::from(vec![
            Span::styled("Quality Score: ", Style::default().fg(theme.primary)),
            Span::raw(format!("{:.1}%", quality_score * 100.0)),
        ]),
        Line::from(vec![
            Span::styled("Status: ", Style::default().fg(theme.primary)),
            Span::styled(status_msg, Style::default().fg(theme.secondary)),
        ]),
    ];
    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Self-Programming Status ")
                .border_style(Style::default().fg(theme.primary)),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

