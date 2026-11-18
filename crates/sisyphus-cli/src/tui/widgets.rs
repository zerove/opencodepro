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

// 实现在 `helpers`；这里 re-export 保持既有调用路径（dialog/renders.rs）。
pub use super::helpers::scroll_pct;

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

// ── 自编程技能徽标 ──────────────────────────────

/// 为自编程注册的技能渲染质量徽标（无质量标签时返回空串）。
///
/// 标签由 `SelfProgrammingManager::register_skill` 写入（`quality:<0-100>`），
/// 因此这里展示的是真实评分而非占位值。
pub fn self_programmed_badge(entry: &crate::tui::app::RemoteSkillEntry) -> String {
    if !entry.is_self_programmed() {
        return String::new();
    }
    match entry.quality_score() {
        Some(q) => format!(" ⚙ auto q{q}% "),
        None => " ⚙ auto ".to_string(),
    }
}
