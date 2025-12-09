
use std::{
    io::{self, Stdout},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender},
        Arc,
    },
    thread,
    time::Duration,
};

use ansi_to_tui::IntoText;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};

use crate::{
    config::Config,
    git::{parse_lfs_mode, run_git_with_lfs, CommandResult, LfsMode},
    theme::Theme,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Cmd,
    Files,
    Log,
    Result,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    CommandLine,
}

pub(crate) enum UiMessage {
    CommandFinished(CommandResult),
}

struct FileEntry {
    status: String,
    path: String,
}

pub struct App {
    config: Config,
    theme: Theme,
    selected_cmd: usize,
    selected_file: usize,
    focus: Focus,
    mode: Mode,
    log_lines: Vec<String>,
    result_lines: Vec<String>,
    log_scroll: u16,
    result_scroll: u16,
    log_view_height: u16,
    result_view_height: u16,
    cmdline: String,
    tx: Sender<UiMessage>,
    rx: Receiver<UiMessage>,
    is_running: bool,
    cancel_flag: Arc<AtomicBool>,
    status_info: String,
    files: Vec<FileEntry>,
}

impl App {
    pub fn new(
        config: Config,
        theme: Theme,
        tx: Sender<UiMessage>,
        rx: Receiver<UiMessage>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Self {
        let mut app = Self {
            config,
            theme,
            selected_cmd: 0,
            selected_file: 0,
            focus: Focus::Cmd,
            mode: Mode::Normal,
            log_lines: vec!["<no output yet>".into()],
            result_lines: vec![],
            log_scroll: 0,
            result_scroll: 0,
            log_view_height: 1,
            result_view_height: 1,
            cmdline: String::new(),
            tx,
            rx,
            is_running: false,
            cancel_flag,
            status_info: String::new(),
            files: Vec::new(),
        };
        app.refresh_status_and_files();
        app
    }

    pub fn run(mut self) -> anyhow::Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        let res = self.event_loop(&mut terminal);
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;
        res
    }

    fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    ) -> anyhow::Result<()> {
        loop {
            self.poll_messages();
            terminal.draw(|f| self.draw(f))?;
            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    let should_quit = match self.mode {
                        Mode::Normal => self.handle_key_normal(key)?,
                        Mode::CommandLine => self.handle_key_cmdline(key)?,
                    };
                    if should_quit {
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    fn poll_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                UiMessage::CommandFinished(res) => {
                    if self.cancel_flag.load(Ordering::Relaxed) {
                        continue;
                    }
                    self.is_running = false;
                    self.log_lines = res.log_lines;
                    self.result_lines = res.result_lines;
                    self.log_scroll = 0;
                    self.result_scroll = 0;
                    self.refresh_status_and_files();
                }
            }
        }
    }

    fn handle_key_normal(&mut self, key: KeyEvent) -> anyhow::Result<bool> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.is_running {
                self.cancel_flag.store(true, Ordering::Relaxed);
                self.is_running = false;
                self.log_lines.push("<canceled by user>".into());
                self.result_lines
                    .push("canceled by user (git process may still finish in background)".into());
                self.refresh_status_and_files();
            }
            return Ok(false);
        }

        if let KeyCode::Char(':') = key.code {
            self.mode = Mode::CommandLine;
            self.cmdline.clear();
            return Ok(false);
        }

        if let KeyCode::Char('q') = key.code {
            return Ok(true);
        }

        match key.code {
            KeyCode::Char('h') => {
                self.focus = match self.focus {
                    Focus::Cmd => Focus::Cmd,
                    Focus::Files => Focus::Cmd,
                    Focus::Log => Focus::Files,
                    Focus::Result => Focus::Log,
                };
                return Ok(false);
            }
            KeyCode::Char('l') => {
                self.focus = match self.focus {
                    Focus::Cmd => Focus::Files,
                    Focus::Files => Focus::Log,
                    Focus::Log => Focus::Result,
                    Focus::Result => Focus::Result,
                };
                return Ok(false);
            }
            _ => {}
        }

        match self.focus {
            Focus::Cmd => self.handle_cmd_keys(key)?,
            Focus::Files => self.handle_file_keys(key)?,
            Focus::Log => self.handle_scroll_keys(key, true)?,
            Focus::Result => self.handle_scroll_keys(key, false)?,
        }

        Ok(false)
    }

    fn handle_key_cmdline(&mut self, key: KeyEvent) -> anyhow::Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.cmdline.clear();
            }
            KeyCode::Enter => {
                let line = self.cmdline.trim().to_string();
                self.cmdline.clear();
                self.mode = Mode::Normal;
                if line.is_empty() {
                    return Ok(false);
                }
                if line == "q" || line == "quit" {
                    return Ok(true);
                }
                match line.as_str() {
                    "stage" => self.run_command_async("add -A".to_string(), LfsMode::None),
                    "unstage" => {
                        self.run_command_async("restore --staged .".to_string(), LfsMode::None)
                    }
                    _ => self.run_command_async(line, LfsMode::None),
                }
            }
            KeyCode::Backspace => {
                self.cmdline.pop();
            }
            KeyCode::Char(c) => {
                self.cmdline.push(c);
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_cmd_keys(&mut self, key: KeyEvent) -> anyhow::Result<()> {
        match key.code {
            KeyCode::Char('j') => {
                if self.selected_cmd + 1 < self.config.commands.len() {
                    self.selected_cmd += 1;
                }
            }
            KeyCode::Char('k') => {
                if self.selected_cmd > 0 {
                    self.selected_cmd -= 1;
                }
            }
            KeyCode::Enter => {
                self.run_selected_command();
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_file_keys(&mut self, key: KeyEvent) -> anyhow::Result<()> {
        match key.code {
            KeyCode::Char('j') => {
                if self.selected_file + 1 < self.files.len() {
                    self.selected_file += 1;
                }
            }
            KeyCode::Char('k') => {
                if self.selected_file > 0 {
                    self.selected_file -= 1;
                }
            }
            KeyCode::Char('s') => {
                self.toggle_stage_selected_file();
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_scroll_keys(&mut self, key: KeyEvent, is_log: bool) -> anyhow::Result<()> {
        let (view_h, lines_len, scroll_ref) = if is_log {
            (self.log_view_height, self.log_lines.len(), &mut self.log_scroll)
        } else {
            (
                self.result_view_height,
                self.result_lines.len(),
                &mut self.result_scroll,
            )
        };

        let max_scroll = lines_len.saturating_sub(view_h as usize) as i32;
        let mut scroll = *scroll_ref as i32;
        let half = (view_h / 2).max(1) as i32;
        let full = view_h.max(1) as i32;

        match key.code {
            KeyCode::PageDown => {
                scroll += full;
            }
            KeyCode::PageUp => {
                scroll -= full;
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                scroll += half;
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                scroll -= half;
            }
            _ => {}
        }

        if scroll < 0 {
            scroll = 0;
        }
        if scroll > max_scroll {
            scroll = max_scroll;
        }

        *scroll_ref = scroll as u16;
        Ok(())
    }

    fn run_selected_command(&mut self) {
        if self.config.commands.is_empty() {
            return;
        }
        let cmd_str = self.config.commands[self.selected_cmd].cmd.clone();
        let lfs_mode = parse_lfs_mode(self.config.commands[self.selected_cmd].lfs.as_ref());
        self.run_command_async(cmd_str, lfs_mode);
    }

    fn toggle_stage_selected_file(&mut self) {
        if self.files.is_empty() {
            return;
        }
        let entry = &self.files[self.selected_file];
        let status = entry.status.as_str();
        let path = entry.path.clone();
        let mut chars = status.chars();
        let x = chars.next().unwrap_or(' ');
        let y = chars.next().unwrap_or(' ');
        let is_untracked = status == "??";
        let is_staged = x != ' ' && !is_untracked;
        let cmd = if is_staged {
            format!("restore --staged {}", path)
        } else {
            format!("add {}", path)
        };
        self.run_command_async(cmd, LfsMode::None);
    }

    fn run_command_async(&mut self, args_str: String, lfs_mode: LfsMode) {
        if self.is_running {
            self.result_lines
                .push("WARN: already running command".into());
            return;
        }
        self.is_running = true;
        self.cancel_flag.store(false, Ordering::Relaxed);
        self.log_lines = vec!["<running...>".into()];
        self.result_lines = vec![format!("$ git {}", args_str), "running...".into()];
        self.log_scroll = 0;
        self.result_scroll = 0;

        let tx = self.tx.clone();
        let git_path = self.config.git_path.clone();
        let cancel_flag = self.cancel_flag.clone();

        thread::spawn(move || {
            let res = run_git_with_lfs(git_path, args_str, lfs_mode, cancel_flag.clone());
            if !cancel_flag.load(Ordering::Relaxed) {
                let _ = tx.send(UiMessage::CommandFinished(res));
            }
        });
    }

    fn refresh_status_and_files(&mut self) {
        let git = &self.config.git_path;
        let repo = std::env::current_dir().unwrap_or_else(|_| ".".into());

        let branch = Command::new(git)
            .arg("rev-parse")
            .arg("--abbrev-ref")
            .arg("HEAD")
            .current_dir(&repo)
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "?".into());

        let status_out = Command::new(git)
            .arg("status")
            .arg("--porcelain=v1")
            .current_dir(&repo)
            .output();

        let mut staged = 0usize;
        let mut unstaged = 0usize;
        let mut untracked = 0usize;
        let mut files = Vec::new();

        if let Ok(o) = status_out {
            if o.status.success() {
                let s = String::from_utf8_lossy(&o.stdout);
                for line in s.lines() {
                    if line.len() < 3 {
                        continue;
                    }
                    let status = line[..2].to_string();
                    let raw_path = line[3..].to_string();
                    if status == "??" {
                        untracked += 1;
                    } else {
                        let x = status.chars().nth(0).unwrap_or(' ');
                        let y = status.chars().nth(1).unwrap_or(' ');
                        if x != ' ' {
                            staged += 1;
                        }
                        if y != ' ' {
                            unstaged += 1;
                        }
                    }
                    files.push(FileEntry {
                        status,
                        path: raw_path,
                    });
                }
            }
        }

        self.status_info = format!(
            "[{}] +{} ~{} ?{}",
            branch, staged, unstaged, untracked
        );
        self.files = files;
        if self.selected_file >= self.files.len() && !self.files.is_empty() {
            self.selected_file = self.files.len() - 1;
        }
        if self.files.is_empty() {
            self.selected_file = 0;
        }
    }

    fn draw(&mut self, f: &mut Frame) {
        let size = f.area();

        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(6), Constraint::Length(1)].as_ref())
            .split(size);

        let top = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(32), Constraint::Min(10)].as_ref())
            .split(vertical[0]);

        let left = top[0];
        let right = top[1];

        let left_split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(7), Constraint::Min(3)].as_ref())
            .split(left);

        let cmd_area = left_split[0];
        let files_area = left_split[1];

        let right_split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(5), Constraint::Length(5)].as_ref())
            .split(right);

        let log_area = right_split[0];
        let result_area = right_split[1];
        let status_area = vertical[1];

        self.log_view_height = log_area.height.saturating_sub(2);
        self.result_view_height = result_area.height.saturating_sub(2);

        let cmd_items: Vec<ListItem> = self
            .config
            .commands
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let marker = if i == self.selected_cmd { "> " } else { "  " };
                let style = if i == self.selected_cmd {
                    Style::default()
                        .fg(self.theme.accent)
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

        let cmd_title = match (self.focus, self.mode) {
            (Focus::Cmd, Mode::Normal) => "CMD [FOCUS]",
            (Focus::Cmd, Mode::CommandLine) => "CMD [FOCUS :]",
            _ => "CMD",
        };

        let cmd_border_style = if matches!(self.focus, Focus::Cmd) {
            Style::default().fg(self.theme.accent)
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

        let file_items: Vec<ListItem> = if self.files.is_empty() {
            vec![ListItem::new(Line::from(Span::raw(
                "<clean or no changes>",
            )))]
        } else {
            self.files
                .iter()
                .enumerate()
                .map(|(i, fe)| {
                    let marker = if i == self.selected_file { "> " } else { "  " };
                    let status = &fe.status;
                    let text = format!("{}[{}] {}", marker, status, fe.path);
                    let mut style = Style::default();
                    if i == self.selected_file {
                        style = style
                            .fg(self.theme.accent)
                            .add_modifier(Modifier::BOLD);
                    }
                    if status == "??" {
                        style = style.add_modifier(Modifier::ITALIC);
                    }
                    ListItem::new(Line::from(Span::styled(text, style)))
                })
                .collect()
        };

        let files_title = match (self.focus, self.mode) {
            (Focus::Files, Mode::Normal) => "FILES [FOCUS] (s:stage/unstage)",
            (Focus::Files, Mode::CommandLine) => "FILES [FOCUS :]",
            _ => "FILES",
        };

        let files_border_style = if matches!(self.focus, Focus::Files) {
            Style::default().fg(self.theme.accent)
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

        let log_title = match (self.focus, self.mode) {
            (Focus::Log, Mode::Normal) => "LOG [FOCUS]",
            (Focus::Log, Mode::CommandLine) => "LOG [FOCUS :]",
            _ => "LOG",
        };

        let log_border_style = if matches!(self.focus, Focus::Log) {
            Style::default().fg(self.theme.accent)
        } else {
            Style::default()
        };

        let log_raw = self.log_lines.join("\n");
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
            .scroll((self.log_scroll, 0));
        f.render_widget(log_widget, log_area);

        let r_title = match (self.focus, self.mode) {
            (Focus::Result, Mode::Normal) => "R [FOCUS]",
            (Focus::Result, Mode::CommandLine) => "R [FOCUS :]",
            _ => "R",
        };

        let r_border_style = if matches!(self.focus, Focus::Result) {
            Style::default().fg(self.theme.accent)
        } else {
            Style::default()
        };

        let r_raw = self.result_lines.join("\n");
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
            .scroll((self.result_scroll, 0));
        f.render_widget(r_widget, result_area);

        let status_line = match self.mode {
            Mode::Normal => {
                let cwd = std::env::current_dir()
                    .ok()
                    .and_then(|p| p.to_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| "?".into());
                Line::from(vec![
                    Span::styled(
                        " -- NORMAL -- ",
                        Style::default().add_modifier(Modifier::REVERSED),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        self.status_info.clone(),
                        Style::default().fg(self.theme.accent),
                    ),
                    Span::raw("  "),
                    Span::raw(cwd),
                ])
            }
            Mode::CommandLine => Line::from(Span::styled(
                format!(":{}", self.cmdline),
                Style::default().add_modifier(Modifier::REVERSED),
            )),
        };

        let status = Paragraph::new(status_line);
        f.render_widget(status, status_area);
    }
}
