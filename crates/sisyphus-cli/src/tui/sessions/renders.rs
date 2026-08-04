use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use super::super::helpers::render_message_with_thought_section;
use super::super::{App, Mode};

pub fn render_sessions(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    if app.sessions.is_empty() {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Sessions ")
            .border_style(Style::default().fg(theme.border));
        let text = Text::from(Line::from(Span::styled(
            "No sessions. Use `sisyphus session create` in shell tab.",
            Style::default().fg(theme.muted),
        )));
        frame.render_widget(Paragraph::new(text).block(block), area);
        return;
    }

    // 详情视图
    if let Some(ref detail) = app.session_detail {
        render_session_detail(frame, area, detail, app.session_detail_scroll, app);
        return;
    }

    // 布局：顶部过滤栏，下方会话列表
    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);

    // 过滤栏
    let filter_style = if app.session_filter.is_empty() {
        Style::default().fg(theme.muted)
    } else {
        Style::default().fg(theme.emphasized)
    };
    let filter_text = if app.session_filter.is_empty() {
        " 🔍 Type to filter sessions..."
    } else {
        &app.session_filter
    };
    let filter_block = Block::default()
        .borders(Borders::ALL)
        .title(" Search ")
        .border_style(Style::default().fg(theme.border));
    let filter_widget = Paragraph::new(Text::from(Line::from(Span::styled(
        filter_text,
        filter_style,
    ))))
    .block(filter_block);
    frame.render_widget(filter_widget, chunks[0]);

    // 会话列表（已过滤）
    let filtered_indices = app.filtered_sessions();
    let visible_count = filtered_indices.len();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Sessions ({visible_count}) "))
        .title_bottom(super::super::helpers::scroll_pct(
            app.session_sel,
            visible_count,
        ))
        .border_style(Style::default().fg(theme.border));

    let items: Vec<ListItem> = filtered_indices
        .iter()
        .map(|&idx| {
            let s = &app.sessions[idx];
            let mut spans = vec![
                Span::raw("  "),
                Span::styled(&s.name, Style::default().fg(theme.fg)),
                Span::styled(
                    format!(" ({} msgs)", s.message_count),
                    Style::default().fg(theme.muted),
                ),
            ];
            // 过滤时显示匹配原因
            if !app.session_filter.is_empty() {
                let q = app.session_filter.to_lowercase();
                if s.name.to_lowercase().contains(&q) {
                    spans.push(Span::styled(" [name]", Style::default().fg(theme.success)));
                } else if s.id.contains(&q) {
                    spans.push(Span::styled(" [id]", Style::default().fg(theme.success)));
                }
            }
            let line = Line::from(spans);
            ListItem::new(line)
        })
        .collect();

    let adj_sel = app.session_sel.min(visible_count.saturating_sub(1));
    let list = List::new(items).block(block).highlight_style(
        Style::default()
            .fg(theme.secondary)
            .add_modifier(Modifier::REVERSED),
    );
    frame.render_stateful_widget(
        list,
        chunks[1],
        &mut ratatui::widgets::ListState::default().with_selected(Some(adj_sel)),
    );
}

fn render_session_detail(
    frame: &mut ratatui::Frame,
    area: Rect,
    session: &crate::tools::Session,
    scroll: usize,
    app: &App,
) {
    let suggestion_count = app.session_suggestions.len() as u16;
    let theme = app.theme();
    let show_suggestions =
        !session.messages.is_empty() && app.pending_op.is_none() && suggestion_count > 0;
    let suggestion_height = if show_suggestions {
        suggestion_count
    } else {
        0
    };

    // 流式事件面板：有事件且会话激活时显示
    let show_stream = !app.stream_events.is_empty() && app.stream_show;
    let stream_height: u16 = if show_stream {
        let max_height = 8u16;
        let count = app.stream_events.len().min(max_height as usize) as u16;
        count + 2 // border + header
    } else {
        0
    };

    let chunks = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(stream_height),
        Constraint::Length(3),
        Constraint::Length(suggestion_height),
    ])
    .split(area);
    let history_area = chunks[0];
    let stream_area = chunks[1];
    let input_area = chunks[2];
    let suggestion_area = chunks[3];

    let time_str = if session.created_at.len() >= 16 {
        &session.created_at[11..16]
    } else {
        &session.created_at
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(
            " {} — {} msgs (created {}) ",
            session.name, session.message_count, time_str
        ))
        .border_style(Style::default().fg(theme.border));

    let mut lines = Vec::new();

    for (idx, msg) in session.messages.iter().enumerate() {
        let is_sel = app.selected_msg == Some(idx);
        let sel_prefix = if is_sel { "▸ " } else { "  " };
        let role_label = match msg.role.as_str() {
            "user" => "You",
            "assistant" => "Sisyphus",
            _ => &msg.role,
        };
        let role_style = match msg.role.as_str() {
            "user" => Style::default().fg(theme.info).add_modifier(Modifier::BOLD),
            "assistant" => Style::default()
                .fg(theme.success)
                .add_modifier(Modifier::BOLD),
            _ => Style::default().fg(theme.muted),
        };
        let ts_style = Style::default().fg(theme.muted);
        let ts = if msg.created_at.len() >= 16 {
            msg.created_at[11..16].to_string()
        } else {
            msg.created_at.clone()
        };
        let sel_style = if is_sel {
            Style::default()
                .fg(theme.secondary)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.muted)
        };
        lines.push(Line::from(vec![
            Span::styled(sel_prefix, sel_style),
            Span::styled(format!("[{}] ", role_label), role_style),
            Span::styled(ts, ts_style),
        ]));
        // 渲染思考块（可折叠）+ 真实回答（始终可见）
        render_message_with_thought_section(&mut lines, &msg.content, app.is_thought_expanded(idx));
        lines.push(Line::from(Span::raw("")));
    }

    // 等待代理响应时的思考指示
    if app.pending_op.is_some() && !session.messages.is_empty() {
        let spinner_frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let frame_idx = (app.pending_spin as usize / 3) % spinner_frames.len();
        let spinner = spinner_frames[frame_idx];
        lines.push(Line::from(vec![
            Span::styled(
                format!("[{spinner}] "),
                Style::default().fg(theme.emphasized),
            ),
            Span::styled("Sisyphus 思考中...", Style::default().fg(theme.emphasized)),
        ]));
    }

    let visible_height = history_area.height.saturating_sub(2);

    // 估算考虑自动换行的显示行数。
    // 不这样做时，换行产生多于逻辑行的显示行数会使 max_scroll 低估。
    let text_area_width = history_area.width.saturating_sub(2) as usize;
    let estimated_display_rows: u16 = if text_area_width == 0 {
        lines.len() as u16
    } else {
        lines
            .iter()
            .map(|l| {
                let w = l.width();
                if w == 0 {
                    1u16
                } else {
                    w.div_ceil(text_area_width) as u16
                }
            })
            .sum()
    };

    let max_scroll = estimated_display_rows.saturating_sub(visible_height);
    let auto_scroll = scroll == usize::MAX;
    let effective_scroll = if auto_scroll {
        max_scroll as usize
    } else {
        scroll.min(max_scroll as usize)
    };

    let paragraph = Paragraph::new(Text::from(lines))
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((effective_scroll as u16, 0));
    frame.render_widget(paragraph, history_area);

    // 保存布局信息用于点击定位消息与滚动钳制
    app.last_detail_area.set(history_area);
    app.last_effective_scroll.set(effective_scroll);
    app.last_detail_max_scroll.set(max_scroll as usize);

    let mode_tag = match app.session_mode {
        Mode::Input => " [INPUT]",
        Mode::Select => " [NORMAL]",
    };
    let input_title: String = if app.pending_op.is_some() {
        " ⏳ 等待回复... ".into()
    } else {
        format!(" Chat{mode_tag} ")
    };
    let input_block = Block::default()
        .borders(Borders::ALL)
        .title(input_title)
        .border_style(if app.session_mode == Mode::Input {
            Style::default().fg(theme.emphasized)
        } else if app.pending_op.is_some() {
            Style::default().fg(theme.error)
        } else {
            Style::default().fg(theme.muted)
        });

    let input_text: &str = if !app.session_input.is_empty() {
        &app.session_input
    } else if app.session_mode == Mode::Input {
        "Type your message and press Enter to send..."
    } else {
        "Press I or Enter to type, Esc to go back"
    };

    let input_style = if app.session_input.is_empty() {
        Style::default().fg(theme.muted)
    } else {
        Style::default().fg(theme.fg)
    };

    let input_widget = Paragraph::new(Line::from(vec![
        Span::styled("User > ", Style::default().fg(theme.info)),
        Span::styled(input_text, input_style),
    ]))
    .block(input_block);

    frame.render_widget(input_widget, input_area);

    if app.session_mode == Mode::Input && !app.session_input.is_empty() {
        let prefix_width = 7;
        let char_count = app.session_input.chars().count();
        let text_before: String = app
            .session_input
            .chars()
            .take(app.session_cursor.min(char_count))
            .collect();
        let cursor_display = text_before.width() as u16;
        frame.set_cursor_position((
            input_area.x + 1 + prefix_width + cursor_display,
            input_area.y + 1,
        ));
    }

    if suggestion_height > 0 && !app.session_suggestions.is_empty() {
        let suggestion_lines: Vec<Line> = app
            .session_suggestions
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let prefix = if i == 0 { "💡 " } else { "   " };
                Line::from(Span::styled(
                    format!("{prefix}{s}"),
                    Style::default().fg(theme.muted),
                ))
            })
            .collect();
        let suggestion_widget = Paragraph::new(Text::from(suggestion_lines))
            .style(Style::default().bg(theme.bg_darker));
        frame.render_widget(suggestion_widget, suggestion_area);
    }

    // ── 搜索覆盖层 ──
    if app.search_active {
        let search_area = Rect {
            x: history_area.x + 1,
            y: history_area.y + history_area.height.saturating_sub(2),
            width: history_area.width.saturating_sub(2),
            height: 2,
        };
        let match_info = if app.search_results.is_empty() {
            if app.search_query.is_empty() {
                " Type to search...".to_string()
            } else {
                " No matches".to_string()
            }
        } else {
            format!(" {} / {} ", app.search_idx + 1, app.search_results.len())
        };
        let query_display = if app.search_query.is_empty() {
            Span::raw("")
        } else {
            Span::styled(
                &app.search_query,
                Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
            )
        };
        let search_fg = if app.search_results.is_empty() && !app.search_query.is_empty() {
            theme.error
        } else {
            theme.info
        };
        let search_bar = Paragraph::new(Line::from(vec![
            Span::styled(" 🔍 ", Style::default().fg(search_fg)),
            query_display,
            Span::styled(match_info, Style::default().fg(theme.muted)),
        ]))
        .style(Style::default().bg(theme.bg_darker));
        frame.render_widget(search_bar, search_area);
    }

    // ── 流式事件面板 ──
    if show_stream && stream_area.height > 2 {
        let stream_block = Block::default()
            .borders(Borders::ALL)
            .title(" ⚡ Bridge Events ")
            .border_style(Style::default().fg(theme.warning));
        let start = app
            .stream_events
            .len()
            .saturating_sub((stream_area.height - 2) as usize);
        let stream_lines: Vec<Line> = app
            .stream_events
            .iter()
            .skip(start)
            .map(|ev| {
                Line::from(vec![
                    Span::styled(
                        format!(" [{}] ", ev.source),
                        Style::default().fg(theme.secondary),
                    ),
                    Span::styled(&ev.message, Style::default().fg(theme.fg)),
                ])
            })
            .collect();
        let stream_widget = Paragraph::new(Text::from(stream_lines)).block(stream_block);
        frame.render_widget(stream_widget, stream_area);
    }
}
