// ── Shared TUI Widget Helpers ─────────────────────────────
//
// Consolidates common rendering patterns to reduce duplication
// across dialog, session, and render modules.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, Borders, List, ListItem},
};

use crate::tui::theme::Theme;

// ── Scroll indicator ───────────────────────────────────

/// Format scroll position as "pos/total (pct%)".
/// Returns empty string when total <= 1 (no scrolling needed).
pub fn scroll_pct(pos: usize, total: usize) -> String {
    if total <= 1 {
        String::new()
    } else {
        let pct =
            (pos as f64 / (total.saturating_sub(1) as f64) * 100.0).round() as u8;
        format!(" {pos}/{total} ({pct}%) ")
    }
}

// ── Generic list renderer ──────────────────────────────

/// Render a selectable list inside a bordered block.
///
/// `items` are the rendered `ListItem`s, `selected` is the current
/// selection index (clamped to `items.len()`).
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
        .highlight_style(
            Style::default()
                .fg(theme.secondary)
                .add_modifier(Modifier::REVERSED),
        );
    let sel = selected.min(count.saturating_sub(1));
    frame.render_stateful_widget(
        list,
        area,
        &mut ratatui::widgets::ListState::default().with_selected(Some(sel)),
    );
}


