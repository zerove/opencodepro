use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Row, Table, Wrap},
};

use super::super::widgets;
use super::super::{App, DevFocus};

// ── 帮助对话框 ────────────────────────────────────────

pub(super) fn render_workspace_list_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Workspaces ")
        .title_bottom(
            " ↑↓ select | Enter switch | d/Del: delete | r: refresh | Esc: close ".to_string(),
        )
        .border_style(Style::default().fg(theme.primary));

    if app.workspaces.is_empty() {
        let text = Text::from(Line::from(Span::styled(
            "No workspaces configured. Press 'r' to refresh.",
            Style::default().fg(theme.muted),
        )));
        frame.render_widget(Paragraph::new(text).block(block), area);
        return;
    }

    let items: Vec<ListItem> = app
        .workspaces
        .iter()
        .enumerate()
        .map(|(i, ws)| {
            let style = if i == app.workspace_sel {
                Style::default()
                    .fg(theme.secondary)
                    .add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(theme.fg)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {} ", ws.name), style.add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!(" (ID: {}) ", ws.id),
                    Style::default().fg(theme.muted),
                ),
                Span::styled(format!("\n  Project: {}", ws.project_id), style),
            ]))
        })
        .collect();

    let list = List::new(items).block(block).highlight_style(
        Style::default()
            .fg(theme.secondary)
            .add_modifier(Modifier::REVERSED),
    );
    frame.render_stateful_widget(
        list,
        area,
        &mut ratatui::widgets::ListState::default().with_selected(Some(app.workspace_sel)),
    );
}

pub(super) fn render_help_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();

    let lines = vec![
        Line::from(Span::styled(
            "  Navigation",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("  Ctrl+K     ", Style::default().fg(theme.secondary)),
            Span::styled("Command palette", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+L     ", Style::default().fg(theme.secondary)),
            Span::styled("Session list", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+H     ", Style::default().fg(theme.secondary)),
            Span::styled("Go home", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+X h   ", Style::default().fg(theme.secondary)),
            Span::styled("Help (leader then h)", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+X p   ", Style::default().fg(theme.secondary)),
            Span::styled("Palette (leader then p)", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+Q     ", Style::default().fg(theme.secondary)),
            Span::styled("Exit (twice to confirm)", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  Tab        ", Style::default().fg(theme.secondary)),
            Span::styled("Toggle all thought blocks", Style::default().fg(theme.fg)),
        ]),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            "  Session Input",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("  Enter      ", Style::default().fg(theme.secondary)),
            Span::styled("Send message", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  Esc        ", Style::default().fg(theme.secondary)),
            Span::styled("Cancel/back", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  ↑ ↓       ", Style::default().fg(theme.secondary)),
            Span::styled("Cycle input history", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  ← →       ", Style::default().fg(theme.secondary)),
            Span::styled("Move cursor", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  Home/End   ", Style::default().fg(theme.secondary)),
            Span::styled("Cursor start/end", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+A/E   ", Style::default().fg(theme.secondary)),
            Span::styled("Start/end of line", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+U     ", Style::default().fg(theme.secondary)),
            Span::styled("Clear line", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+W     ", Style::default().fg(theme.secondary)),
            Span::styled("Delete word backward", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+K     ", Style::default().fg(theme.secondary)),
            Span::styled("Delete to end of line", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  Backspace  ", Style::default().fg(theme.secondary)),
            Span::styled("Delete before cursor", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  Delete     ", Style::default().fg(theme.secondary)),
            Span::styled("Delete at cursor", Style::default().fg(theme.fg)),
        ]),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            "  Slash Commands (in input)",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("  /help      ", Style::default().fg(theme.secondary)),
            Span::styled("Open this help", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  /dev       ", Style::default().fg(theme.secondary)),
            Span::styled("Open dev dialog", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  /home      ", Style::default().fg(theme.secondary)),
            Span::styled("Go to home", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  /sessions  ", Style::default().fg(theme.secondary)),
            Span::styled("Go to sessions", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  /clear     ", Style::default().fg(theme.secondary)),
            Span::styled("Clear input", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  /new       ", Style::default().fg(theme.secondary)),
            Span::styled("Create new session", Style::default().fg(theme.fg)),
        ]),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            "  Session History",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("  ↑ ↓       ", Style::default().fg(theme.secondary)),
            Span::styled("Scroll messages", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  PgUp/PgDn  ", Style::default().fg(theme.secondary)),
            Span::styled("Scroll 10 lines", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  g / G      ", Style::default().fg(theme.secondary)),
            Span::styled("Top / bottom", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  t          ", Style::default().fg(theme.secondary)),
            Span::styled("Toggle last thought block", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+F     ", Style::default().fg(theme.secondary)),
            Span::styled("Search messages", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  y          ", Style::default().fg(theme.secondary)),
            Span::styled("Copy last assistant reply", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  Y          ", Style::default().fg(theme.secondary)),
            Span::styled("Copy selected message", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  C          ", Style::default().fg(theme.secondary)),
            Span::styled("Copy all messages", Style::default().fg(theme.fg)),
        ]),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            "  Session List",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("  r          ", Style::default().fg(theme.secondary)),
            Span::styled("Refresh", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  d / Del    ", Style::default().fg(theme.secondary)),
            Span::styled("Delete session", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  R          ", Style::default().fg(theme.secondary)),
            Span::styled("Rename session", Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("  c          ", Style::default().fg(theme.secondary)),
            Span::styled("Create new session", Style::default().fg(theme.fg)),
        ]),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            "Pro tip: Type / in session input for slash commands",
            Style::default().fg(theme.info),
        )),
    ];

    let line_count = lines.len() as u16;
    let visible_height = area.height.saturating_sub(2);
    let max_scroll = line_count.saturating_sub(visible_height);
    let scroll = app.help_scroll.min(max_scroll as usize);

    let title_bottom = if max_scroll > 0 {
        format!(
            " ↑↓/scroll: {} | Esc/q to close ",
            widgets::scroll_pct(scroll, max_scroll as usize)
        )
    } else {
        " Esc/q to close ".into()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Help ")
        .title_bottom(title_bottom)
        .border_style(Style::default().fg(theme.primary));

    let para = Paragraph::new(Text::from(lines))
        .block(block)
        .scroll((scroll as u16, 0));
    frame.render_widget(para, area);
}

// ── 会话列表对话框 ───────────────────────────────

pub(super) fn render_session_list_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let items: Vec<ListItem> = app
        .sessions
        .iter()
        .map(|s| {
            ListItem::new(Line::from(vec![
                Span::styled(&s.name, Style::default().fg(theme.fg)),
                Span::styled(
                    format!(" ({} msgs)", s.message_count),
                    Style::default().fg(theme.muted),
                ),
            ]))
        })
        .collect();
    widgets::render_list(
        frame,
        area,
        items,
        app.session_sel,
        " Sessions ".into(),
        &theme,
    );
}

// ── 待办对话框 ─────────────────────────────────────

pub(super) fn render_todos_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Todos ")
        .border_style(Style::default().fg(theme.primary));

    if app.todo_items.is_empty() {
        let text = Text::from(Line::from(Span::styled(
            "No todos yet. Press a to add one.",
            Style::default().fg(theme.muted),
        )));
        frame.render_widget(Paragraph::new(text).block(block), area);
        return;
    }

    let items: Vec<ListItem> = app
        .todo_items
        .iter()
        .map(|item| {
            let checkbox = match item.status {
                crate::tools::TodoStatus::Pending => "[ ]",
                crate::tools::TodoStatus::InProgress => "[~]",
                crate::tools::TodoStatus::Completed => "[✓]",
                crate::tools::TodoStatus::Cancelled => "[✗]",
            };
            let style = match item.status {
                crate::tools::TodoStatus::Completed => Style::default()
                    .fg(theme.success)
                    .add_modifier(Modifier::DIM),
                crate::tools::TodoStatus::Cancelled => {
                    Style::default().fg(theme.error).add_modifier(Modifier::DIM)
                }
                crate::tools::TodoStatus::InProgress => Style::default().fg(theme.primary),
                crate::tools::TodoStatus::Pending => Style::default().fg(theme.fg),
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{} ", checkbox), style),
                Span::styled(&item.content, style),
            ]))
        })
        .collect();

    let list = List::new(items).block(block).highlight_style(
        Style::default()
            .fg(theme.secondary)
            .add_modifier(Modifier::REVERSED),
    );
    frame.render_stateful_widget(
        list,
        area,
        &mut ratatui::widgets::ListState::default().with_selected(Some(app.todo_sel)),
    );
}

// ── LSP 对话框 ───────────────────────────────────────

pub(super) fn render_lsp_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let filtered: Vec<&crate::tools::lsp::LspDiagnostic> = if app.dialog_query.is_empty() {
        app.lsp_diagnostics.iter().collect()
    } else {
        let q = app.dialog_query.to_lowercase();
        app.lsp_diagnostics
            .iter()
            .filter(|d| d.message.to_lowercase().contains(&q))
            .collect()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(
            " LSP Diagnostics [{} {}] ",
            app.lsp_language, app.lsp_server
        ))
        .title_bottom(format!(
            " {} / {} | Type to filter... ",
            filtered.len(),
            app.lsp_diagnostics.len()
        ))
        .border_style(Style::default().fg(theme.primary));

    let items: Vec<ListItem> = filtered
        .iter()
        .map(|d| {
            let sev = d
                .severity
                .map(crate::tools::lsp::severity_name)
                .unwrap_or("?");
            let style = match sev {
                "error" => Style::default().fg(theme.error),
                "warning" => Style::default().fg(theme.warning),
                "info" => Style::default().fg(theme.info),
                _ => Style::default().fg(theme.muted),
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:>6} ", sev), style),
                Span::styled(&d.message, Style::default().fg(theme.fg)),
            ]))
        })
        .collect();

    let list = List::new(items).block(block);
    frame.render_stateful_widget(
        list,
        area,
        &mut ratatui::widgets::ListState::default().with_selected(Some(app.lsp_scroll)),
    );
}

// ── SOP 对话框 ────────────────────────────────────────

pub(super) fn render_sop_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let filtered: Vec<&crate::bridge::SopInfo> = if app.sop_query.is_empty() {
        app.sop_sops.iter().collect()
    } else {
        let q = app.sop_query.to_lowercase();
        app.sop_sops
            .iter()
            .filter(|s| s.sop_id.to_lowercase().contains(&q))
            .collect()
    };

    let items: Vec<ListItem> = if filtered.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "Press r to refresh / connect to bridge",
            Style::default().fg(theme.muted),
        )))]
    } else {
        filtered
            .iter()
            .map(|sop| {
                ListItem::new(Line::from(vec![
                    Span::styled(&sop.sop_id, Style::default().fg(theme.fg)),
                    Span::styled(
                        format!(" ({} nodes)", sop.node_count),
                        Style::default().fg(theme.muted),
                    ),
                ]))
            })
            .collect()
    };
    widgets::render_list(frame, area, items, app.sop_sel, " SOP List ".into(), &theme);
}

// ── 路线图对话框 ────────────────────────────────────

pub(super) fn render_roadmap_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Project Roadmap & Strategic Proposals ")
        .border_style(Style::default().fg(theme.primary));

    let layout = Layout::vertical([
        Constraint::Percentage(60), // Roadmap Progress
        Constraint::Length(1),      // Separator
        Constraint::Percentage(40), // Strategic Proposals
    ])
    .split(area);

    // 1. 路线图进度区
    let roadmap_lines = if let Some(p) = &app.roadmap_projection {
        let mut lines = vec![Line::from(Span::styled(
            "── Current Progress ──",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ))];
        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(p, Style::default().fg(theme.fg))));
        lines
    } else {
        vec![Line::from(Span::styled(
            "No roadmap data available. Press 'r' to fetch.",
            Style::default().fg(theme.muted),
        ))]
    };

    let roadmap_para = Paragraph::new(Text::from(roadmap_lines))
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false })
        .scroll((app.roadmap_scroll as u16, 0));
    frame.render_widget(roadmap_para, layout[0]);

    // 2. 分隔线
    frame.render_widget(
        Paragraph::new(
            "────────────────────────────────────────────────────────────────────────────────",
        )
        .style(Style::default().fg(theme.muted)),
        layout[1],
    );

    // 3. 战略提案区
    let proposal_lines = if let Some(b) = &app.roadmap_bottlenecks {
        let mut lines = vec![Line::from(Span::styled(
            "── Pending Strategic Proposals ──",
            Style::default()
                .fg(theme.warning)
                .add_modifier(Modifier::BOLD),
        ))];
        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(b, Style::default().fg(theme.fg))));
        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(
            "  [ Enter ] Approve selected proposal",
            Style::default().fg(theme.info),
        )));
        lines
    } else {
        vec![Line::from(Span::styled(
            "No pending strategic proposals.",
            Style::default().fg(theme.muted),
        ))]
    };

    let prop_para = Paragraph::new(Text::from(proposal_lines))
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false });
    frame.render_widget(prop_para, layout[2]);

    // 在布局外围渲染主块
    frame.render_widget(block, area);
}

// ── 确认对话框 ─────────────────────────────────────

pub(super) fn render_confirm_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Confirm ")
        .border_style(Style::default().fg(theme.warning));

    let text = Text::from(vec![
        Line::from(Span::styled(
            "Delete this session?",
            Style::default().fg(theme.fg),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Press y to confirm, n or Esc to cancel",
            Style::default().fg(theme.muted),
        )),
    ]);

    let para = Paragraph::new(text).block(block);
    frame.render_widget(para, area);
}

pub(super) fn render_skill_market_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Skill Market ")
        .title_bottom(
            " r: refresh available | i: refresh installed | Enter: install/uninstall | Esc: close "
                .to_string(),
        )
        .border_style(Style::default().fg(theme.primary));

    let layout = Layout::vertical([
        Constraint::Length(1),      // Status
        Constraint::Percentage(50), // Available
        Constraint::Length(1),      // Separator
        Constraint::Percentage(49), // Installed
    ])
    .split(area);

    // 1. 状态
    let status_text = Text::from(Line::from(Span::styled(
        format!(" Status: {} ", app.skill_market_status),
        Style::default().fg(theme.info),
    )));
    frame.render_widget(
        Paragraph::new(status_text).block(Block::default().borders(Borders::NONE)),
        layout[0],
    );

    // 2. 可用技能
    let available_items: Vec<ListItem> = app
        .skill_market_available
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let is_sel = i == app.skill_market_sel;
            let style = if is_sel {
                Style::default()
                    .fg(theme.secondary)
                    .add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(theme.fg)
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" [+] {} ", s.name),
                    style.add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" v{} by {} ", s.version, s.author),
                    Style::default().fg(theme.muted),
                ),
                Span::styled(format!("\n  {}", s.description), style),
            ]))
        })
        .collect();

    let avail_block = Block::default()
        .borders(Borders::NONE)
        .title(" Available Skills ");
    let avail_list = List::new(available_items).block(avail_block);
    frame.render_stateful_widget(
        avail_list,
        layout[1],
        &mut ratatui::widgets::ListState::default().with_selected(Some(app.skill_market_sel)),
    );

    // 3. 分隔线
    frame.render_widget(
        Paragraph::new(
            "────────────────────────────────────────────────────────────────────────────────",
        )
        .style(Style::default().fg(theme.muted)),
        layout[2],
    );

    // 4. 已安装技能
    let installed_offset = app.skill_market_available.len();
    let installed_items: Vec<ListItem> = app
        .skill_market_installed
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let is_sel = (i + installed_offset) == app.skill_market_sel;
            let style = if is_sel {
                Style::default()
                    .fg(theme.secondary)
                    .add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(theme.fg)
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" [-] {} ", s.name),
                    style.add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" v{} by {} ", s.version, s.author),
                    Style::default().fg(theme.muted),
                ),
                Span::styled(format!("\n  {}", s.description), style),
            ]))
        })
        .collect();

    let inst_block = Block::default()
        .borders(Borders::NONE)
        .title(" Installed Skills ");
    let inst_list = List::new(installed_items).block(inst_block);
    frame.render_stateful_widget(
        inst_list,
        layout[3],
        &mut ratatui::widgets::ListState::default()
            .with_selected(Some(app.skill_market_sel.saturating_sub(installed_offset))),
    );

    frame.render_widget(block, area);
}

pub(super) fn render_rename_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App, name: &str) {
    let theme = app.theme();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Rename Session ")
        .border_style(Style::default().fg(theme.primary));
    let [_, inner, _] = Layout::vertical([
        Constraint::Percentage(50),
        Constraint::Length(3),
        Constraint::Percentage(50),
    ])
    .areas(area);

    let display = if name.is_empty() {
        " (empty — cancels) "
    } else {
        name
    };
    let input = Paragraph::new(Line::from(Span::styled(
        format!(" {} ", display),
        Style::default().fg(theme.fg),
    )))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.secondary)),
    );
    let hint = Paragraph::new(Line::from(Span::styled(
        " Enter to confirm | Esc to cancel ",
        Style::default().fg(theme.muted),
    )));

    let layout = Layout::vertical([Constraint::Length(3), Constraint::Length(1)]).split(inner);
    frame.render_widget(block, area);
    frame.render_widget(input, layout[0]);
    frame.render_widget(hint, layout[1]);
}

// ── 开发对话框 ────────────────────────────────────────

pub(super) fn render_dev_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Dev Agent ")
        .border_style(Style::default().fg(theme.primary));

    let focus_style = |focused: bool| {
        if focused {
            Style::default()
                .fg(theme.secondary)
                .add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(theme.fg)
        }
    };

    // 报告可用时显示
    if let Some(report) = &app.dev_report {
        let lines: Vec<Line> = report
            .lines()
            .map(|l| {
                let style = if l.starts_with("✅") || l.starts_with("✔") {
                    Style::default().fg(theme.success)
                } else if l.starts_with("❌") || l.starts_with("⚠") {
                    Style::default().fg(theme.error)
                } else {
                    Style::default().fg(theme.fg)
                };
                Line::from(Span::styled(l.to_string(), style))
            })
            .collect();

        let para = Paragraph::new(Text::from(lines))
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(para, area);
        return;
    }

    // 显示运行状态
    if app.dev_status == "running" {
        let text = Text::from(vec![
            Line::from(Span::styled(
                "  Running developer agent...",
                Style::default().fg(theme.primary),
            )),
            Line::from(Span::styled(
                "  This may take a few minutes.",
                Style::default().fg(theme.muted),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Press Esc to cancel (agent continues in background)",
                Style::default().fg(theme.muted),
            )),
        ]);
        let para = Paragraph::new(text).block(block);
        frame.render_widget(para, area);
        return;
    }

    // 显示输入表单
    let mut lines = Vec::new();

    // 目标
    let goal_label = Span::styled("Goal:", focus_style(app.dev_focus == DevFocus::Goal));
    let goal_val = if app.dev_goal.is_empty() {
        Span::styled(" <type your dev goal>", Style::default().fg(theme.muted))
    } else {
        Span::styled(&app.dev_goal, Style::default().fg(theme.fg))
    };
    lines.push(Line::from(vec![goal_label, goal_val]));
    lines.push(Line::from(""));

    // 上下文
    let ctx_label = Span::styled("Context:", focus_style(app.dev_focus == DevFocus::Context));
    let ctx_val = if app.dev_context.is_empty() {
        Span::styled(
            " <optional project context>",
            Style::default().fg(theme.muted),
        )
    } else {
        Span::styled(&app.dev_context, Style::default().fg(theme.fg))
    };
    lines.push(Line::from(vec![ctx_label, ctx_val]));
    lines.push(Line::from(""));

    // 并行
    let par_checked = if app.dev_parallel { "[x]" } else { "[ ]" };
    let par_label = Span::styled(
        format!("Parallel: {par_checked}  (run steps concurrently, max 4)"),
        focus_style(app.dev_focus == DevFocus::Parallel),
    );
    lines.push(Line::from(par_label));
    lines.push(Line::from(""));

    // 恢复
    let res_checked = if app.dev_resume { "[x]" } else { "[ ]" };
    let res_label = Span::styled(
        format!("Resume: {res_checked}  (skip completed steps from checkpoint)"),
        focus_style(app.dev_focus == DevFocus::Resume),
    );
    lines.push(Line::from(res_label));
    lines.push(Line::from(""));

    // 运行按钮
    let run_style = if app.dev_focus == DevFocus::Run {
        Style::default()
            .fg(theme.secondary)
            .add_modifier(Modifier::REVERSED)
    } else {
        Style::default().fg(theme.success)
    };
    lines.push(Line::from(Span::styled(" [ Run Dev Agent ] ", run_style)));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " Tab/Shift+Tab: navigate  Enter: run  Space: toggle  Esc: close",
        Style::default().fg(theme.muted),
    )));

    let para = Paragraph::new(Text::from(lines)).block(block);
    frame.render_widget(para, area);
}

pub(super) fn render_trace_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App, sop_id: &str) {
    let theme = app.theme();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Execution Trace - {} ", sop_id))
        .title_bottom(format!(
            "{}{}{}",
            if app.sop_trace_detail.is_some() {
                "Enter: hide output | "
            } else {
                "Enter: show output | "
            },
            format_args!(
                "↑↓/PgUp/PgDn scroll: {} | g/G top/bottom | Esc close ",
                widgets::scroll_pct(app.sop_trace_scroll, app.sop_trace.len()),
            ),
            "",
        ))
        .border_style(Style::default().fg(theme.primary));

    if app.sop_trace.is_empty() {
        let para = Paragraph::new("No trace data available. Please run the SOP first.")
            .block(block)
            .style(Style::default().fg(theme.muted));
        frame.render_widget(para, area);
        return;
    }

    // 布局：上方表格，下方可选输出详情
    let (table_area, detail_area) = if app.sop_trace_detail.is_some() {
        let chunks =
            Layout::vertical([Constraint::Percentage(60), Constraint::Percentage(40)]).split(area);
        (chunks[0], Some(chunks[1]))
    } else {
        (area, None)
    };

    let rows: Vec<Row> = app
        .sop_trace
        .iter()
        .map(|ev| {
            let status_style = match ev.status.as_str() {
                "ok" => Style::default().fg(theme.success),
                "error" => Style::default().fg(theme.error),
                _ => Style::default().fg(theme.fg),
            };
            Row::new(vec![
                Span::styled(&ev.node_id, Style::default().fg(theme.fg)),
                Span::styled(&ev.status, status_style),
                Span::styled(
                    format!("{}ms", ev.duration),
                    Style::default().fg(theme.muted),
                ),
                Span::styled(ev.fuel.to_string(), Style::default().fg(theme.muted)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(30),
            Constraint::Percentage(20),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ],
    )
    .header(Row::new(vec![
        Span::styled(
            "Node",
            Style::default()
                .fg(theme.secondary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Status",
            Style::default()
                .fg(theme.secondary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Duration",
            Style::default()
                .fg(theme.secondary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Fuel",
            Style::default()
                .fg(theme.secondary)
                .add_modifier(Modifier::BOLD),
        ),
    ]))
    .block(Block::default().borders(Borders::NONE))
    .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    frame.render_stateful_widget(
        table,
        table_area,
        &mut ratatui::widgets::TableState::default().with_selected(Some(app.sop_trace_scroll)),
    );

    // 输出详情面板
    if let Some(ref output) = app.sop_trace_detail {
        let detail_block = Block::default()
            .borders(Borders::ALL)
            .title(" Node Output ")
            .border_style(Style::default().fg(theme.secondary));
        let detail_lines: Vec<Line> = if output.is_empty() {
            vec![Line::from(Span::styled(
                "(no output)",
                Style::default().fg(theme.muted),
            ))]
        } else {
            output
                .lines()
                .map(|l| Line::from(Span::styled(l.to_string(), Style::default().fg(theme.fg))))
                .collect()
        };
        let detail_para = Paragraph::new(Text::from(detail_lines))
            .block(detail_block)
            .wrap(Wrap { trim: false });
        frame.render_widget(detail_para, detail_area.unwrap_or(area));
    }

    frame.render_widget(block, area);
}
