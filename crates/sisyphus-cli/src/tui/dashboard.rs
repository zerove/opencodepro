// v2 认知循环仪表盘面板
// 显示引擎运行状态、异常仪表盘和混沌工程快照。

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Gauge, Row, Table},
};

use super::App;

const STATUS_LABELS: &[&str] = &[
    "Cognitive Loop",
    "Anomaly Detector",
    "Chaos Engineering",
    "Quality Monitor",
    "Self-Programming",
];

pub(crate) fn handle_dashboard_key(_key: crossterm::event::KeyEvent, _app: &mut App) {
    // 仪表盘目前只读；未来：T/F 过滤异常类型
}

pub(crate) fn render_dashboard(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();

    // 分为顶部卡片与底部详情
    let [top_area, bottom_area] = Layout::vertical([
        Constraint::Length(8), // status cards + gauges
        Constraint::Min(6),    // anomaly / chaos detail table
    ])
    .areas(area);

    // ── 顶部：一行 5 张状态卡 ──────────────────────
    let card_widths = [
        Constraint::Length(22),
        Constraint::Length(24),
        Constraint::Length(22),
        Constraint::Length(22),
        Constraint::Length(22),
    ];
    let cards = Layout::horizontal(card_widths).split(top_area);

    let statuses = render_status_cards(app);
    for (i, card_area) in cards.iter().enumerate() {
        if i < statuses.len() {
            let (block, gauge_pct) = &statuses[i];
            let [label_area, bar_area] =
                Layout::vertical([Constraint::Length(3), Constraint::Length(3)]).areas(*card_area);
            frame.render_widget(block.clone(), label_area);
            let gauge = Gauge::default()
                .gauge_style(
                    Style::default()
                        .fg(theme.primary)
                        .bg(theme.border)
                        .add_modifier(Modifier::BOLD),
                )
                .percent(*gauge_pct);
            frame.render_widget(gauge, bar_area);
        }
    }

    // ── 底部：最近异常 / 混沌实验 / 状态表 ──
    let bottom_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.primary))
        .title(" Operational Status ");
    let inner = bottom_block.inner(bottom_area);
    frame.render_widget(bottom_block, bottom_area);

    let [left_table, right_table] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(inner);

    render_anomaly_table(frame, left_table, app);
    render_chaos_table(frame, right_table, app);
}

/// 返回 5 张状态卡各自的 (block, gauge_percent)。
fn render_status_cards(app: &App) -> Vec<(Block<'static>, u16)> {
    let theme = app.theme();
    let cog_active = app.dashboard_cog_active;
    let anomaly_count = app.dashboard_anomaly_count;
    let chaos_running = app.dashboard_chaos_running;
    // 质量与自编程卡片由已安装技能（含自编程注册的 quality:<n> 标签）驱动；
    // 无数据时显式显示 "n/a" 而不是伪造一个分数。
    let sp_stats = app.dashboard_sp_stats;
    let quality_score = sp_stats.average_quality;
    let sp_count = sp_stats.self_programmed;

    let states: [(u16, String); 5] = [
        (
            if cog_active { 75 } else { 10 },
            if cog_active { "active" } else { "idle" }.to_string(),
        ),
        (
            ((anomaly_count as u16).min(10).saturating_sub(1) * 10).min(100),
            if anomaly_count > 0 { "alert" } else { "normal" }.to_string(),
        ),
        (
            if chaos_running { 90 } else { 10 },
            if chaos_running { "running" } else { "stopped" }.to_string(),
        ),
        (
            quality_score.unwrap_or(0),
            match quality_score {
                Some(q) if q > 70 => format!("good {q}%"),
                Some(q) => format!("poor {q}%"),
                None => "n/a".to_string(),
            },
        ),
        (
            if sp_count > 0 { 100 } else { 10 },
            if sp_count > 0 {
                format!("{sp_count} skills")
            } else {
                "idle".to_string()
            },
        ),
    ];

    STATUS_LABELS
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let (pct, status) = &states[i];
            let fg = match status.as_str() {
                "active" | "running" | "normal" => theme.success,
                "alert" | "pending" => theme.warning,
                "idle" | "stopped" | "n/a" => theme.muted,
                s if s.starts_with("good") => theme.success,
                s if s.starts_with("poor") => theme.warning,
                s if s.ends_with("skills") => theme.secondary,
                _ => theme.fg,
            };
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border))
                .title(Line::from(vec![
                    Span::styled(
                        *label,
                        Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                    Span::styled(status.clone(), Style::default().fg(fg)),
                ]))
                .title_alignment(ratatui::layout::Alignment::Center)
                .to_owned();
            (block, *pct)
        })
        .collect()
}

fn render_anomaly_table(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let rows: Vec<Row> = if app.dashboard_anomaly_count > 0 {
        vec![Row::new(vec![
            Cell::from(Span::styled("detected", Style::default().fg(theme.fg))),
            Cell::from(Span::styled(
                format!("{} anomalies", app.dashboard_anomaly_count),
                Style::default().fg(theme.warning),
            )),
            Cell::from(Span::styled("active", Style::default().fg(theme.warning))),
        ])]
    } else {
        // 无桥接数据时使用占位演示行
        vec![
            Row::new(vec![
                Cell::from(Span::styled("—", Style::default().fg(theme.muted))),
                Cell::from(Span::styled("—", Style::default().fg(theme.muted))),
                Cell::from(Span::styled("—", Style::default().fg(theme.muted))),
            ]),
            Row::new(vec![
                Cell::from(Span::styled("fuel_spike", Style::default().fg(theme.fg))),
                Cell::from(Span::styled("node_compile", Style::default().fg(theme.fg))),
                Cell::from(Span::styled("medium", Style::default().fg(theme.warning))),
            ]),
            Row::new(vec![
                Cell::from(Span::styled("fail_rate", Style::default().fg(theme.fg))),
                Cell::from(Span::styled("node_test", Style::default().fg(theme.fg))),
                Cell::from(Span::styled("low", Style::default().fg(theme.success))),
            ]),
        ]
    };

    let table = Table::new(
        rows,
        [
            Constraint::Length(16),
            Constraint::Length(16),
            Constraint::Length(10),
        ],
    )
    .block(Block::default().title(" Anomalies (last 5) "));
    frame.render_widget(table, area);
}

fn render_chaos_table(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    let rows: Vec<Row> = if app.dashboard_chaos_running {
        vec![Row::new(vec![
            Cell::from(Span::styled("active", Style::default().fg(theme.warning))),
            Cell::from(Span::styled("running", Style::default().fg(theme.success))),
            Cell::from(Span::styled("—", Style::default().fg(theme.muted))),
        ])]
    } else {
        vec![Row::new(vec![
            Cell::from(Span::styled("none", Style::default().fg(theme.muted))),
            Cell::from(Span::styled("—", Style::default().fg(theme.muted))),
            Cell::from(Span::styled("—", Style::default().fg(theme.muted))),
        ])]
    };

    let table = Table::new(
        rows,
        [
            Constraint::Length(16),
            Constraint::Length(14),
            Constraint::Length(10),
        ],
    )
    .block(Block::default().title(" Chaos Experiments "));
    frame.render_widget(table, area);
}
