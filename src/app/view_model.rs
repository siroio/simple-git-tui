use anyhow::Result;
use std::env;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, Sender},
};
use std::thread;

use crossterm::{
    event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};

use crate::config::{CommandConfig, Config, LayoutConfig};
use crate::git::{
    CommandResult, LfsMode, RepoFile, RepoStatus, load_repo_status, parse_args_line,
    parse_lfs_mode, run_git_with_lfs,
};
use crate::theme::Theme;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Cmd,
    Files,
    Log,
    Result,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    CommandLine,
}

pub enum UiMessage {
    CommandFinished(CommandResult),
}

pub struct ViewModel {
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
    status: RepoStatus,
}

impl ViewModel {
    pub fn new(
        config: Config,
        theme: Theme,
        tx: Sender<UiMessage>,
        rx: Receiver<UiMessage>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Self {
        let status = load_repo_status(&config.git_path, &current_repo_path());
        Self {
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
            status,
        }
    }

    pub fn poll_messages(&mut self) {
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
                    self.refresh_repo_status();
                }
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> anyhow::Result<bool> {
        if key.kind != KeyEventKind::Press {
            return Ok(false);
        }

        match self.mode {
            Mode::Normal => self.handle_key_normal(key),
            Mode::CommandLine => self.handle_key_cmdline(key),
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
                self.refresh_repo_status();
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
                    "stage" => self.run_command("add -A".to_string(), LfsMode::None, false),
                    "unstage" => {
                        self.run_command("restore --staged .".to_string(), LfsMode::None, false)
                    }
                    _ => {
                        let interactive = self.requires_interactive(&line, None);
                        self.run_command(line, LfsMode::None, interactive);
                    }
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
                if self.selected_file + 1 < self.status.files.len() {
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
            (
                self.log_view_height,
                self.log_lines.len(),
                &mut self.log_scroll,
            )
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
        let cmd_cfg = &self.config.commands[self.selected_cmd];
        let cmd_str = cmd_cfg.cmd.clone();
        let lfs_mode = parse_lfs_mode(cmd_cfg.lfs.as_ref());
        let interactive = self.requires_interactive(&cmd_str, Some(cmd_cfg));
        self.run_command(cmd_str, lfs_mode, interactive);
    }

    fn toggle_stage_selected_file(&mut self) {
        if self.status.files.is_empty() {
            return;
        }
        let entry = &self.status.files[self.selected_file];
        let status = entry.status.as_str();
        let mut chars = status.chars();
        let x = chars.next().unwrap_or(' ');
        let is_untracked = status == "??";
        let is_staged = x != ' ' && !is_untracked;
        let operands = entry
            .operands()
            .iter()
            .map(|p| format!("\"{}\"", p))
            .collect::<Vec<_>>()
            .join(" ");

        let cmd = if is_staged {
            format!("restore --staged -- {}", operands)
        } else {
            format!("add -- {}", operands)
        };
        self.run_command(cmd, LfsMode::None, false);
    }

    fn run_command(&mut self, args_str: String, lfs_mode: LfsMode, interactive: bool) {
        if interactive {
            self.run_command_interactive(args_str);
        } else {
            self.run_command_async(args_str, lfs_mode);
        }
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

    fn run_command_interactive(&mut self, args_str: String) {
        if self.is_running {
            self.result_lines
                .push("WARN: already running command".into());
            return;
        }

        self.is_running = true;
        self.log_lines = vec!["<interactive command: terminal will switch>".into()];
        self.result_lines = vec![format!("$ git {}", args_str)];

        let git_path = self.config.git_path.clone();
        let repo = current_repo_path();
        let args = parse_args_line(&args_str);

        let exit_code = (|| -> Result<i32> {
            disable_raw_mode().ok();
            {
                let mut stdout = std::io::stdout();
                execute!(stdout, LeaveAlternateScreen)?;
            }

            let status = std::process::Command::new(&git_path)
                .args(&args)
                .current_dir(&repo)
                .status()?;

            {
                let mut stdout = std::io::stdout();
                execute!(stdout, EnterAlternateScreen)?;
            }
            enable_raw_mode().ok();

            Ok(status.code().unwrap_or(-1))
        })();

        match exit_code {
            Ok(code) => self.result_lines.push(format!("git exit code: {}", code)),
            Err(e) => self
                .result_lines
                .push(format!("ERROR: failed interactive git: {e}")),
        }

        self.is_running = false;
        self.refresh_repo_status();
    }

    fn refresh_repo_status(&mut self) {
        self.status = load_repo_status(&self.config.git_path, &current_repo_path());
        if self.selected_file >= self.status.files.len() && !self.status.files.is_empty() {
            self.selected_file = self.status.files.len() - 1;
        }
        if self.status.files.is_empty() {
            self.selected_file = 0;
        }
    }

    pub fn update_viewport(&mut self, log_height: u16, result_height: u16) {
        self.log_view_height = log_height.max(1);
        self.result_view_height = result_height.max(1);
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn layout(&self) -> &LayoutConfig {
        &self.config.layout
    }

    pub fn commands(&self) -> &[CommandConfig] {
        &self.config.commands
    }

    pub fn focus(&self) -> Focus {
        self.focus
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn selected_cmd(&self) -> usize {
        self.selected_cmd
    }

    pub fn selected_file(&self) -> usize {
        self.selected_file
    }

    pub fn files(&self) -> &[RepoFile] {
        &self.status.files
    }

    pub fn log_lines(&self) -> &[String] {
        &self.log_lines
    }

    pub fn result_lines(&self) -> &[String] {
        &self.result_lines
    }

    pub fn log_scroll(&self) -> u16 {
        self.log_scroll
    }

    pub fn result_scroll(&self) -> u16 {
        self.result_scroll
    }

    pub fn status_summary(&self) -> String {
        self.status.summary()
    }

    pub fn cmdline(&self) -> &str {
        &self.cmdline
    }

    fn requires_interactive(&self, args_str: &str, cfg: Option<&CommandConfig>) -> bool {
        if cfg.map(|c| c.interactive).unwrap_or(false) {
            return true;
        }
        self.is_commit_needing_editor(args_str)
    }

    fn is_commit_needing_editor(&self, args_str: &str) -> bool {
        let parts = parse_args_line(args_str);
        if parts.is_empty() {
            return false;
        }

        if parts[0] != "commit" {
            return false;
        }

        let has_message_flag = parts
            .iter()
            .any(|p| p == "-m" || p == "--message" || p.starts_with("--message="));

        !has_message_flag
    }
}

fn current_repo_path() -> std::path::PathBuf {
    env::current_dir().unwrap_or_else(|_| ".".into())
}
