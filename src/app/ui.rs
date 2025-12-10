use ansi_to_tui::IntoText;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use super::{App, Focus, Mode};

pub(super) fn draw(app: &mut App, f: &mut Frame<'_>) {
    let size = f.area();

    let lw = app.config.layout.cmd_width as u16;
    let fh = app.config.layout.files_height as u16;
    let rh = app.config.layout.result_height as u16;

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(1)].as_ref())
        .split(size);

    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(lw), Constraint::Min(10)].as_ref())
        .split(vertical[0]);

    let left = top[0];
    let right = top[1];

    let left_split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(fh), Constraint::Min(3)].as_ref())
        .split(left);

    let cmd_area = left_split[0];
    let files_area = left_split[1];

    let right_split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(rh)].as_ref())
        .split(right);

    let log_area = right_split[0];
    let result_area = right_split[1];
    let status_area = vertical[1];

    app.log_view_height = log_area.height.saturating_sub(2);
    app.result_view_height = result_area.height.saturating_sub(2);

    let cmd_items: Vec<ListItem> = app
        .config
        .commands
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let marker = if i == app.selected_cmd { "> " } else { "  " };
            let style = if i == app.selected_cmd {
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(
                        format!("{}{}", marker, c.name),
                        style,
            )))
        })
    .collect();

    let cmd_title = match (app.focus, app.mode) {
        (Focus::Cmd, Mode::Normal) => "CMD [FOCUS]",
        (Focus::Cmd, Mode::CommandLine) => "CMD [FOCUS :]",
        _ => "CMD",
    };

    let cmd_border_style = if matches!(app.focus, Focus::Cmd) {
        Style::default().fg(app.theme.accent)
    } else {
        Style::default()
    };

    let cmd_list = List::new(cmd_items).block(
        Block::default()
        .title(cmd_title)
        .borders(Borders::ALL)
        .border_style(cmd_border_style),
    );
    f.render_widget(cmd_list, cmd_area);

    let file_items: Vec<ListItem> = if app.files.is_empty() {
        vec![ListItem::new(Line::from(Span::raw(
                    "<clean or no changes>",
        )))]
    } else {
        app.files
            .iter()
            .enumerate()
            .map(|(i, fe)| {
                let marker = if i == app.selected_file { "? " } else { "  " };
                let status = fe.status.as_str();

                let clean_path = fe.path.replace('"', "");

                let display_name = clean_path
                    .rsplit(|c| c == '/' || c == '\\')
                    .next()
                    .unwrap_or(clean_path.as_str());

                let mut chars = status.chars();
                let x = chars.next().unwrap_or(' ');
                let y = chars.next().unwrap_or(' ');
                let is_untracked = status == "??";
                let is_staged = x != ' ' && !is_untracked;
                let has_unstaged = y != ' ';

                let status_label = format!("[{}]", status);

                let text = format!("{}{} {}", marker, status_label, display_name);

                let mut style = Style::default();

                if is_staged {
                    style = style.fg(app.theme.accent);
                }

                if is_untracked {
                    style = style.add_modifier(Modifier::ITALIC);
                }

                if i == app.selected_file {
                    style = style.add_modifier(Modifier::BOLD);
                }

                if is_staged && has_unstaged {
                     style = style.add_modifier(Modifier::UNDERLINED);
                }

                ListItem::new(Line::from(Span::styled(text, style)))
            })
        .collect()
    };

    let files_title = match (app.focus, app.mode) {
        (Focus::Files, Mode::Normal) => "FILES [FOCUS] (s:stage/unstage)",
        (Focus::Files, Mode::CommandLine) => "FILES [FOCUS :]",
        _ => "FILES",
    };

    let files_border_style = if matches!(app.focus, Focus::Files) {
        Style::default().fg(app.theme.accent)
    } else {
        Style::default()
    };

    let files_list = List::new(file_items).block(
        Block::default()
        .title(files_title)
        .borders(Borders::ALL)
        .border_style(files_border_style),
    );
    f.render_widget(files_list, files_area);

    let log_title = match (app.focus, app.mode) {
        (Focus::Log, Mode::Normal) => "LOG [FOCUS]",
        (Focus::Log, Mode::CommandLine) => "LOG [FOCUS :]",
        _ => "LOG",
    };

    let log_border_style = if matches!(app.focus, Focus::Log) {
        Style::default().fg(app.theme.accent)
    } else {
        Style::default()
    };

    let log_raw = app.log_lines.join("\n");
    let log_text: Text = log_raw
        .as_str()
        .into_text()
        .unwrap_or_else(|_| Text::raw(log_raw));

    let log_widget = Paragraph::new(log_text)
        .block(
            Block::default()
            .title(log_title)
            .borders(Borders::ALL)
            .border_style(log_border_style),
        )
        .scroll((app.log_scroll, 0));
    f.render_widget(log_widget, log_area);

    let r_title = match (app.focus, app.mode) {
        (Focus::Result, Mode::Normal) => "R [FOCUS]",
        (Focus::Result, Mode::CommandLine) => "R [FOCUS :]",
        _ => "R",
    };

    let r_border_style = if matches!(app.focus, Focus::Result) {
        Style::default().fg(app.theme.accent)
    } else {
        Style::default()
    };

    let r_raw = app.result_lines.join("\n");
    let r_text: Text = r_raw
        .as_str()
        .into_text()
        .unwrap_or_else(|_| Text::raw(r_raw));

    let r_widget = Paragraph::new(r_text)
        .block(
            Block::default()
            .title(r_title)
            .borders(Borders::ALL)
            .border_style(r_border_style),
        )
        .scroll((app.result_scroll, 0));
    f.render_widget(r_widget, result_area);

    let status_line = match app.mode {
        Mode::Normal => {
            let cwd = std::env::current_dir()
                .ok()
                .and_then(|p| p.to_str().map(|s| s.to_string()))
                .unwrap_or_else(|| "?".into());

            let file_path = app
                .files
                .get(app.selected_file)
                .map(|f| f.path.replace('"', ""));

            let max_len = status_area.width.saturating_sub(40) as usize;
            let file_display = file_path.map(|p| {
                if max_len == 0 || p.len() <= max_len {
                    p
                } else {
                    let start = p.len() - max_len;
                    format!("乧{}", &p[start..])
                }
            });

            let mut spans: Vec<Span> = Vec::new();
            spans.push(Span::styled(
                    " -- NORMAL -- ",
                    Style::default().add_modifier(Modifier::REVERSED),
            ));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                    app.status_info.clone(),
                    Style::default().fg(app.theme.accent),
            ));
            spans.push(Span::raw("  "));
            spans.push(Span::raw(cwd));

            if let Some(fp) = file_display {
                spans.push(Span::raw("  |  "));
                spans.push(Span::raw("file: "));
                spans.push(Span::styled(
                        fp,
                        Style::default().fg(app.theme.accent),
                ));
            }

            Line::from(spans)
        }
        Mode::CommandLine => Line::from(Span::styled(
                format!(":{}", app.cmdline),
                Style::default().add_modifier(Modifier::REVERSED),
        )),
    };

    let status = Paragraph::new(status_line);
    f.render_widget(status, status_area);
}
