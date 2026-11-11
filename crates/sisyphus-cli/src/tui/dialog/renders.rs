use ratatui::{
    layout::{Constraint, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Table, Wrap},
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
                Style::default().fg(theme.secondary).add_modifier(Modifier::REVERSED)
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

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().fg(theme.secondary).add_modifier(Modifier::REVERSED));
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
            Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
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
            Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
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
            Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
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
            Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
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
            Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
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

    let para = Paragraph::new(Text::from(lines)).block(block).scroll((scroll as u16, 0));
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
                crate::tools::TodoStatus::Completed => {
                    Style::default().fg(theme.success).add_modifier(Modifier::DIM)
                },
                crate::tools::TodoStatus::Cancelled => {
                    Style::default().fg(theme.error).add_modifier(Modifier::DIM)
                },
                crate::tools::TodoStatus::InProgress => Style::default().fg(theme.primary),
                crate::tools::TodoStatus::Pending => Style::default().fg(theme.fg),
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{} ", checkbox), style),
                Span::styled(&item.content, style),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().fg(theme.secondary).add_modifier(Modifier::REVERSED));
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
        app.lsp_diagnostics.iter().filter(|d| d.message.to_lowercase().contains(&q)).collect()
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
            let sev = d.severity.map(crate::tools::lsp::severity_name).unwrap_or("?");
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
        app.sop_sops.iter().filter(|s| s.sop_id.to_lowercase().contains(&q)).collect()
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
            Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
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
            Style::default().fg(theme.warning).add_modifier(Modifier::BOLD),
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

pub(super) fn render_confirm_dialog(
    frame: &mut ratatui::Frame,
    area: Rect,
    app: &App,
    target: super::ConfirmTarget,
) {
    let theme = app.theme();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Confirm ")
        .border_style(Style::default().fg(theme.warning));

    let question = match target {
        super::ConfirmTarget::Session => "Delete this session?",
        super::ConfirmTarget::Todo => "Delete this todo?",
        super::ConfirmTarget::Workspace => "Delete this workspace?",
    };

    let text = Text::from(vec![
        Line::from(Span::styled(question, Style::default().fg(theme.fg))),
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
    let sp = app.dashboard_sp_stats;
    let sp_summary = if sp.self_programmed == 0 {
        String::new()
    } else {
        match sp.average_quality {
            Some(q) => format!(
                " | self-programmed: {} (avg quality {q}%)",
                sp.self_programmed
            ),
            None => format!(" | self-programmed: {}", sp.self_programmed),
        }
    };
    let status_text = Text::from(Line::from(Span::styled(
        format!(" Status: {} {sp_summary}", app.skill_market_status),
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
                Style::default().fg(theme.secondary).add_modifier(Modifier::REVERSED)
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

    let avail_block = Block::default().borders(Borders::NONE).title(" Available Skills ");
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
                Style::default().fg(theme.secondary).add_modifier(Modifier::REVERSED)
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
                Span::styled(
                    crate::tui::widgets::self_programmed_badge(s),
                    Style::default().fg(theme.success),
                ),
                Span::styled(format!("\n  {}", s.description), style),
            ]))
        })
        .collect();

    let inst_block = Block::default().borders(Borders::NONE).title(" Installed Skills ");
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
        Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.secondary)),
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
            Style::default().fg(theme.secondary).add_modifier(Modifier::REVERSED)
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

        let para = Paragraph::new(Text::from(lines)).block(block).wrap(Wrap { trim: false });
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
        Style::default().fg(theme.secondary).add_modifier(Modifier::REVERSED)
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

pub(super) fn render_cognitive_trace_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let source_label = if app.cog_trace_history_mode {
        "SQL history (persisted)"
    } else {
        "live buffer (memory)"
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Cognitive Trace (Decision Chain) — {source_label} "))
        .title_bottom(format!(
            "{} | Enter: details | r: refresh | h: {}/history | ↑↓/PgUp/PgDn: {} | g/G top/bottom | Esc close ",
            app.cog_trace_status,
            if app.cog_trace_history_mode { "live" } else { "history" },
            widgets::scroll_pct(app.cog_trace_scroll, app.cog_trace.len()),
        ))
        .border_style(Style::default().fg(theme.primary));

    if app.cog_trace.is_empty() {
        let text = Text::from(Line::from(Span::styled(
            "No cognitive decisions recorded yet. Run a goal/SOP first, then press 'r'.",
            Style::default().fg(theme.muted),
        )));
        frame.render_widget(Paragraph::new(text).block(block), area);
        return;
    }

    // 布局：上方表格，下方可选详情
    let (table_area, detail_area) = if app.cog_trace_detail.is_some() {
        let chunks =
            Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)]).split(area);
        (chunks[0], Some(chunks[1]))
    } else {
        (area, None)
    };

    let rows: Vec<Row> = app
        .cog_trace
        .iter()
        .map(|row| {
            let time = if app.cog_trace_history_mode {
                let ms = row.timestamp_ms as i64;
                chrono::DateTime::from_timestamp_millis(ms)
                    .map(|dt| dt.format("%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| format!("{ms}ms"))
            } else {
                format!("{:.1}s", row.timestamp_ms as f64 / 1000.0)
            };
            let (tag, summary, tag_style) = cognitive_event_summary(&row.event, &theme);
            Row::new(vec![
                Span::styled(time, Style::default().fg(theme.muted)),
                Span::styled(&row.sop_id, Style::default().fg(theme.fg)),
                Span::styled(tag, tag_style),
                Span::styled(summary, Style::default().fg(theme.fg)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(9),
            Constraint::Percentage(14),
            Constraint::Percentage(16),
            Constraint::Percentage(61),
        ],
    )
    .header(Row::new(vec![
        Span::styled(
            "Time",
            Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "SOP",
            Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Event",
            Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Summary",
            Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD),
        ),
    ]))
    .block(Block::default().borders(Borders::NONE))
    .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    frame.render_stateful_widget(
        table,
        table_area,
        &mut ratatui::widgets::TableState::default().with_selected(Some(app.cog_trace_scroll)),
    );

    // 决策详情面板
    if let Some(ref detail) = app.cog_trace_detail {
        let detail_block = Block::default()
            .borders(Borders::ALL)
            .title(" Decision Detail ")
            .border_style(Style::default().fg(theme.secondary));
        let detail_lines: Vec<Line> = detail
            .lines()
            .map(|l| Line::from(Span::styled(l.to_string(), Style::default().fg(theme.fg))))
            .collect();
        let detail_para =
            Paragraph::new(Text::from(detail_lines)).block(detail_block).wrap(Wrap { trim: false });
        frame.render_widget(detail_para, detail_area.unwrap_or(area));
    }

    frame.render_widget(block, area);
}

/// 将认知事件归结为 (标签, 摘要, 标签样式)，供表格单行显示。
fn cognitive_event_summary(
    event: &sisyphus_types::ExecutionEvent,
    theme: &super::super::theme::Theme,
) -> (String, String, Style) {
    use sisyphus_types::ExecutionEvent;
    match event {
        ExecutionEvent::NodeStarted { node_name } => (
            "START".into(),
            format!("Node started: {node_name}"),
            Style::default().fg(theme.primary),
        ),
        ExecutionEvent::NodeCompleted { node_name, .. } => (
            "DONE".into(),
            format!("Node completed: {node_name}"),
            Style::default().fg(theme.success),
        ),
        ExecutionEvent::NodeFailed { node_name, .. } => (
            "FAIL".into(),
            format!("Node failed: {node_name}"),
            Style::default().fg(theme.error),
        ),
        ExecutionEvent::ToolCall {
            tool_id,
            phase,
            duration_ms,
            success,
            error,
        } => {
            let style = match success {
                Some(false) => theme.error,
                Some(true) => theme.success,
                None => theme.secondary,
            };
            let mut summary = format!("Tool {tool_id}: {phase}");
            if let Some(ms) = duration_ms {
                summary.push_str(&format!(" ({ms}ms)"));
            }
            if let Some(err) = error.as_deref() {
                summary.push_str(&format!(" — {err}"));
            }
            ("TOOL".into(), summary, Style::default().fg(style))
        },
        ExecutionEvent::TokenUsage {
            model,
            prompt_tokens,
            completion_tokens,
            total_tokens,
        } => (
            "TOKENS".into(),
            format!(
                "{}: prompt {prompt_tokens} + completion {completion_tokens} = {total_tokens}",
                model.as_deref().unwrap_or("llm")
            ),
            Style::default().fg(theme.secondary),
        ),
        ExecutionEvent::ExecutionComplete { .. } => (
            "EXEC".into(),
            "Execution complete".into(),
            Style::default().fg(theme.success),
        ),
        ExecutionEvent::CognitivePlan { plan } => (
            "PLAN".into(),
            format!(
                "Cognitive plan for '{}' ({} steps)",
                plan.goal,
                plan.steps.len()
            ),
            Style::default().fg(theme.primary),
        ),
        ExecutionEvent::CognitiveStep {
            step,
            message,
            metrics,
        } => {
            let metrics_txt = metrics.as_ref().map_or_else(String::new, |m| {
                format!(" | dev {:.2} | conf {:.2}", m.deviation, m.confidence)
            });
            (
                "STEP".into(),
                format!("[{step}] {message}{metrics_txt}"),
                Style::default().fg(theme.secondary),
            )
        },
        ExecutionEvent::MicroReflection {
            node_name,
            prediction,
            actual,
            decision,
            ..
        } => {
            let delta = if prediction == actual {
                "match"
            } else {
                "drift"
            };
            let decision_style = if decision.contains("reject") {
                theme.error
            } else if delta == "drift" {
                theme.warning
            } else {
                theme.success
            };
            (
                "REFLECT".into(),
                format!(
                    "{node_name}: {delta} | {} -> {} | {}",
                    truncate(prediction, 24),
                    truncate(actual, 24),
                    decision,
                ),
                Style::default().fg(decision_style),
            )
        },
        ExecutionEvent::CognitiveComplete { .. } => (
            "DONE".into(),
            "Cognitive loop converged".into(),
            Style::default().fg(theme.success),
        ),
    }
}

/// 截断过长的文本，保留尾部省略号。
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
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
            Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Status",
            Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Duration",
            Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Fuel",
            Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD),
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
        let detail_para =
            Paragraph::new(Text::from(detail_lines)).block(detail_block).wrap(Wrap { trim: false });
        frame.render_widget(detail_para, detail_area.unwrap_or(area));
    }

    frame.render_widget(block, area);
}

/// 认知决策健康摘要对话框（跨会话聚合）。
pub(super) fn render_cognitive_summary_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    use ratatui::layout::Constraint;
    let theme = app.theme();

    let Some(summary) = &app.cog_summary else {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Cognitive Health Summary (cross-session) ")
            .title_bottom(format!(
                "{} | r: refresh | Esc close ",
                app.cog_summary_status
            ))
            .border_style(Style::default().fg(theme.primary));
        let text = Text::from(Line::from(Span::styled(
            "No cognitive summary yet. Run a goal/SOP first, then press 'r'.",
            Style::default().fg(theme.muted),
        )));
        frame.render_widget(Paragraph::new(text).block(block), area);
        return;
    };

    let (left, right) = {
        let chunks = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);
        (chunks[0], chunks[1])
    };

    let dev_style = |d: f64| {
        if d < 0.3 {
            theme.success
        } else if d < 0.6 {
            theme.warning
        } else {
            theme.error
        }
    };
    let conf_style = |c: f64| {
        if c >= 0.8 {
            theme.success
        } else if c >= 0.6 {
            theme.warning
        } else {
            theme.error
        }
    };

    let mut lines: Vec<Line> = vec![Line::from(Span::styled(
        "─ Decisions ─",
        Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
    ))];
    lines.push(Line::from(Span::raw(format!(
        "Total:  {}",
        summary.total_decisions
    ))));
    lines.push(Line::from(Span::raw(format!(
        "Steps:  {}    Reflections:  {}",
        summary.step_count, summary.reflection_count
    ))));
    lines.push(Line::from(Span::raw("")));
    lines.push(Line::from(Span::styled(
        "─ Quality (means) ─",
        Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(vec![
        Span::styled("Deviation:  ", Style::default().fg(theme.muted)),
        Span::styled(
            format!("{:.3}", summary.avg_deviation),
            Style::default().fg(dev_style(summary.avg_deviation)),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Confidence: ", Style::default().fg(theme.muted)),
        Span::styled(
            format!("{:.3}", summary.avg_confidence),
            Style::default().fg(conf_style(summary.avg_confidence)),
        ),
    ]));
    lines.push(Line::from(Span::raw(format!(
        "Stability:  {:.3}    Info gain:  {:.3}",
        summary.avg_stability, summary.avg_info_gain
    ))));
    lines.push(Line::from(Span::raw(format!(
        "Token cost: {}",
        summary.total_token_cost
    ))));

    let left_para =
        Paragraph::new(Text::from(lines)).block(Block::default().borders(Borders::NONE));
    frame.render_widget(left_para, left);

    // 右侧：决策分布 + 时间范围 + 健康判定
    let mut right_lines: Vec<Line> = vec![Line::from(Span::styled(
        "─ Reflection decisions ─",
        Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
    ))];
    let total_refl = summary.reflection_count.max(1);
    let bar_w = 18_usize;
    if summary.decision_counts.is_empty() {
        right_lines.push(Line::from(Span::styled(
            "no micro-reflections recorded",
            Style::default().fg(theme.muted),
        )));
    } else {
        let mut sorted: Vec<(&String, &u64)> = summary.decision_counts.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));
        for (decision, count) in sorted {
            let ratio = *count as f64 / total_refl as f64;
            let filled = (ratio * bar_w as f64).round() as usize;
            let bar: String =
                "█".repeat(filled.min(bar_w)) + &"░".repeat(bar_w - filled.min(bar_w));
            right_lines.push(Line::from(Span::raw(format!(
                "{decision:<9} {:>4}  {bar} {:.0}%",
                count,
                ratio * 100.0
            ))));
        }
    }
    right_lines.push(Line::from(Span::raw("")));
    right_lines.push(Line::from(Span::styled(
        "─ Window ─",
        Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
    )));
    let fmt_ts = |ms: u128| -> String {
        chrono::DateTime::from_timestamp_millis(ms as i64)
            .map(|dt| dt.format("%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| format!("{ms}ms"))
    };
    right_lines.push(Line::from(Span::raw(format!(
        "from: {}",
        fmt_ts(summary.first_timestamp_ms)
    ))));
    right_lines.push(Line::from(Span::raw(format!(
        "to:   {}",
        fmt_ts(summary.last_timestamp_ms)
    ))));
    right_lines.push(Line::from(Span::raw("")));
    right_lines.push(Line::from(Span::styled(
        "─ Health verdict ─",
        Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
    )));
    let (verdict, vstyle) = if summary.total_decisions == 0 {
        ("NO DATA".to_string(), theme.muted)
    } else if summary.avg_deviation < 0.3 && summary.avg_confidence >= 0.6 {
        ("STABLE".to_string(), theme.success)
    } else if summary.avg_deviation < 0.6 && summary.avg_confidence >= 0.4 {
        ("CAUTION".to_string(), theme.warning)
    } else {
        ("DRIFTING".to_string(), theme.error)
    };
    right_lines.push(Line::from(vec![
        Span::styled(
            format!("{verdict:<10}"),
            Style::default().fg(vstyle).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "low deviation + high confidence = stable loop",
            Style::default().fg(theme.muted),
        ),
    ]));

    let right_para =
        Paragraph::new(Text::from(right_lines)).block(Block::default().borders(Borders::NONE));
    frame.render_widget(right_para, right);
}

/// 渲染 `:health` 对话框（bridge `/health/deep` 启动健康度）。
///
/// 顶端状态条（OK / DEGRADED）+ 组件清单表格 + raw JSON 详情。
pub(super) fn render_health_dialog(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    use ratatui::layout::{Constraint, Direction, Layout};

    let theme = app.theme();

    let verdict = if app.health_all_ok { "OK" } else { "DEGRADED" };
    let verdict_style = if app.health_all_ok {
        Style::default().fg(theme.success).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.error).add_modifier(Modifier::BOLD)
    };

    // 1) 顶部状态条
    let mut status_lines = vec![Line::from(vec![
        Span::raw("Overall: "),
        Span::styled(verdict, verdict_style),
        Span::raw(format!("  ({})", app.health_status)),
    ])];
    if let Some(t) = app.health_last_refresh {
        let elapsed = t.elapsed().as_secs();
        status_lines.push(Line::from(Span::styled(
            format!("Last refresh: {elapsed}s ago  (r: refresh, Esc: close)"),
            Style::default().fg(theme.muted),
        )));
    } else {
        status_lines.push(Line::from(Span::styled(
            "No refresh yet — press r to probe /health/deep".to_string(),
            Style::default().fg(theme.muted),
        )));
    }

    let status_block = Block::default().borders(Borders::ALL).title(" Bridge Health (`:health`) ");
    let status_para = Paragraph::new(Text::from(status_lines)).block(status_block);
    frame.render_widget(status_para, area);

    // 内部布局：状态条 + 组件表 + 详情
    let inner = area.inner(Margin::new(1, 1));
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(5),
            Constraint::Length(8),
        ])
        .split(inner);

    // 2) 组件清单（status / path 等）
    let header = Row::new(vec![
        Cell::from("Component")
            .style(Style::default().fg(theme.muted).add_modifier(Modifier::BOLD)),
        Cell::from("Status").style(Style::default().fg(theme.muted).add_modifier(Modifier::BOLD)),
        Cell::from("Detail").style(Style::default().fg(theme.muted).add_modifier(Modifier::BOLD)),
    ]);

    let rows: Vec<Row> = if app.health_components.is_empty() {
        vec![Row::new(vec![
            Cell::from("—"),
            Cell::from("—"),
            Cell::from("no data; press r"),
        ])]
    } else {
        // 排序：ok → absent → degraded，便于快速读出异常
        let mut keys: Vec<&String> = app.health_components.keys().collect();
        keys.sort_by_key(|k| {
            let status = app
                .health_components
                .get(*k)
                .and_then(|v| v.get("status"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match status {
                "ok" => 0,
                "absent" => 1,
                _ => 2,
            }
        });
        keys.into_iter()
            .map(|k| {
                let v = app.health_components.get(k).cloned().unwrap_or(serde_json::Value::Null);
                let status =
                    v.get("status").and_then(|s| s.as_str()).unwrap_or("unknown").to_string();
                // 优先显示组件特定字段：
                //   session_db → path
                //   token_budget → usage/cap/exhausted 摘要
                //   其他 → 整 JSON
                let detail = if let Some(p) = v.get("path").and_then(|p| p.as_str()) {
                    p.to_string()
                } else if k == "token_budget" {
                    let usage = v.get("usage").and_then(|n| n.as_u64()).unwrap_or(0);
                    let cap = v.get("cap").and_then(|n| n.as_u64());
                    let exhausted = v.get("exhausted").and_then(|b| b.as_bool()).unwrap_or(false);
                    match cap {
                        Some(c) => format!(
                            "{}/{} tokens{}",
                            usage,
                            c,
                            if exhausted { " (exhausted)" } else { "" }
                        ),
                        None => format!("{} tokens (unbounded)", usage),
                    }
                } else {
                    v.to_string()
                };

                let status_style = match status.as_str() {
                    "ok" => Style::default().fg(theme.success),
                    "degraded" => Style::default().fg(theme.error),
                    "absent" => Style::default().fg(theme.muted),
                    _ => Style::default(),
                };
                Row::new(vec![
                    Cell::from(k.clone()),
                    Cell::from(status).style(status_style),
                    Cell::from(detail),
                ])
            })
            .collect()
    };

    let table = Table::new(
        rows,
        [
            Constraint::Length(16),
            Constraint::Length(12),
            Constraint::Min(20),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title(" Components "));
    frame.render_widget(table, chunks[1]);

    // 3) 原始 JSON 预览（clamp scroll）
    let raw_text =
        serde_json::to_string_pretty(&serde_json::Value::Object(app.health_components.clone()))
            .unwrap_or_else(|_| "<failed to serialize>".into());
    let raw_lines: Vec<Line> = raw_text
        .lines()
        .skip(app.health_scroll)
        .map(|l| Line::from(Span::raw(l.to_string())))
        .collect();
    let raw_para = Paragraph::new(Text::from(raw_lines))
        .block(Block::default().borders(Borders::ALL).title(" Raw components JSON "));
    frame.render_widget(raw_para, chunks[2]);
}
