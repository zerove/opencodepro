use crossterm::event::KeyCode;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use super::helpers::scroll_pct;
use super::{App, Mode, LspInputMode};

pub async fn handle_lsp_key(code: KeyCode, app: &mut App) {
    match code {
        KeyCode::Char('f') if app.mode == Mode::Select => {
            app.lsp_input_mode = LspInputMode::FilePath;
            app.mode = Mode::Input;
            app.lsp_input.clear();
        }
        KeyCode::Char('l') if app.mode == Mode::Select => {
            app.lsp_input_mode = LspInputMode::Language;
            app.mode = Mode::Input;
            app.lsp_input.clear();
        }
        KeyCode::Char('s') if app.mode == Mode::Select => {
            app.lsp_input_mode = LspInputMode::Server;
            app.mode = Mode::Input;
            app.lsp_input.clear();
        }
        KeyCode::Char('a') if app.mode == Mode::Select => {
            app.lsp_input_mode = LspInputMode::ServerArgs;
            app.mode = Mode::Input;
            app.lsp_input.clear();
        }
        KeyCode::Enter => match app.mode {
            Mode::Input => handle_lsp_input(app),
            Mode::Select | Mode::Scroll => {
                app.mode = Mode::Input;
                app.lsp_input_mode = LspInputMode::FilePath;
                app.lsp_input.clear();
            }
        },
        KeyCode::Esc if app.mode == Mode::Input => {
            app.mode = Mode::Select;
            app.lsp_input.clear();
        }
        KeyCode::Tab => app.mode = Mode::Scroll,
        KeyCode::Up | KeyCode::Char('k') => {
            app.lsp_scroll = app.lsp_scroll.saturating_sub(1)
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.lsp_scroll = app.lsp_scroll.saturating_add(1)
        }
        KeyCode::PageUp => app.lsp_scroll = app.lsp_scroll.saturating_sub(10),
        KeyCode::PageDown => app.lsp_scroll = app.lsp_scroll.saturating_add(10),
        KeyCode::Home | KeyCode::Char('g') => app.lsp_scroll = 0,
        KeyCode::End | KeyCode::Char('G') => app.lsp_scroll = usize::MAX,
        KeyCode::Char('1') => {
            app.lsp_language = "rust".into();
            app.lsp_server = "rust-analyzer".into();
            app.lsp_server_args = "".into();
        }
        KeyCode::Char('2') => {
            app.lsp_language = "typescript".into();
            app.lsp_server = "typescript-language-server".into();
            app.lsp_server_args = "--stdio".into();
        }
        KeyCode::Char('3') => {
            app.lsp_language = "python".into();
            app.lsp_server = "pylsp".into();
            app.lsp_server_args = "".into();
        }
        _ => {}
    }
}

fn handle_lsp_input(app: &mut App) {
    match app.lsp_input_mode {
        LspInputMode::FilePath => {
            app.lsp_input_mode = LspInputMode::Language;
            app.lsp_input.clear();
        }
        LspInputMode::Language => {
            app.lsp_language = app.lsp_input.clone();
            app.lsp_input_mode = LspInputMode::Server;
            app.lsp_input.clear();
        }
        LspInputMode::Server => {
            app.lsp_server = app.lsp_input.clone();
            app.lsp_input_mode = LspInputMode::ServerArgs;
            app.lsp_input.clear();
        }
        LspInputMode::ServerArgs => {
            app.lsp_server_args = app.lsp_input.clone();
            app.lsp_input.clear();
            app.mode = Mode::Select;
        }
    }
}

pub fn render_lsp(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let theme = app.theme();
    use ratatui::widgets::ListState;

    let [input_area, info_area, list_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(area);

    let field_label = match app.lsp_input_mode {
        LspInputMode::FilePath => " File Path ",
        LspInputMode::Language => " Language ID ",
        LspInputMode::Server => " Server Command ",
        LspInputMode::ServerArgs => " Server Args (space-separated) ",
    };
    let input_block = Block::default()
        .borders(Borders::ALL)
        .title(field_label)
        .border_style(if app.mode == Mode::Input {
            Style::default().fg(theme.primary)
        } else {
            Style::default().fg(theme.muted)
        });
    let input_paragraph = Paragraph::new(app.lsp_input.as_str())
        .block(input_block)
        .style(Style::default());
    frame.render_widget(input_paragraph, input_area);

    if app.mode == Mode::Input && app.lsp_input_mode == LspInputMode::FilePath {
        frame.set_cursor_position((
            input_area.x + 1 + app.lsp_input.len() as u16,
            input_area.y + 1,
        ));
    }

    // Info line
    let info_text = if app.lsp_status == "error" && !app.lsp_last_error.is_empty() {
        format!(
            "Error: {} | Server: {} {}",
            app.lsp_last_error, app.lsp_server, app.lsp_server_args
        )
    } else {
        format!(
            "Status: {} | Lang: {} | Server: {} {}",
            app.lsp_status, app.lsp_language, app.lsp_server, app.lsp_server_args
        )
    };
    let info_style = match app.lsp_status.as_str() {
        "idle" | "clean" => Style::default().fg(theme.success),
        "running" => Style::default().fg(theme.primary),
        "issues" => Style::default().fg(theme.warning),
        _ => Style::default().fg(theme.error),
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(info_text, info_style))),
        info_area,
    );

    // Diagnostics list
    let list_block = Block::default()
        .borders(Borders::ALL)
        .title(" Diagnostics ")
        .title_bottom(scroll_pct(app.lsp_scroll, app.lsp_diagnostics.len()))
         .border_style(Style::default().fg(theme.muted));

    let items: Vec<ListItem> = app
        .lsp_diagnostics
        .iter()
        .map(|d| {
            let sev = d
                .severity
                .map(sisyphus_tools::lsp::severity_name)
                .unwrap_or("?");
            let style = match sev {
                 "error" => Style::default().fg(theme.error),
                 "warning" => Style::default().fg(theme.warning),
                 "info" => Style::default().fg(theme.info),
                 _ => Style::default().fg(theme.muted),
            };
            let line = Line::from(vec![
                Span::styled(format!("{:>6} ", sev), style),
                Span::styled(
                    format!(
                        "{}:{}:{}",
                        d.range.start.line, d.range.start.character, d.range.end.character
                    ),
                     Style::default().fg(theme.muted),
                ),
                Span::raw(" "),
                 Span::styled(&d.message, Style::default().fg(theme.fg)),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).block(list_block);
    frame.render_stateful_widget(
        list,
        list_area,
        &mut ListState::default().with_selected(Some(app.lsp_scroll)),
    );
}
