use anyhow::Result;
use std::env;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, Sender},
};
use std::thread;

use crossterm::{
    event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
};

use crate::config::{CommandConfig, Config, LayoutConfig};
use crate::git::{
    CommandResult, LfsMode, RepoFile, RepoStatus, load_repo_status, parse_args_line,
    parse_lfs_mode, repo_root, run_git_with_lfs,
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
    needs_full_redraw: bool,
    repo_root: PathBuf,
    tx: Sender<UiMessage>,
    rx: Receiver<UiMessage>,
    is_running: bool,
    cancel_flag: Arc<AtomicBool>,
    status: RepoStatus,
    pending_discard: Option<usize>,
}

impl ViewModel {
    pub fn new(
        config: Config,
        theme: Theme,
        tx: Sender<UiMessage>,
        rx: Receiver<UiMessage>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Self {
        let cwd = current_repo_path();
        let repo_root = repo_root(&config.git_path, &cwd);
        let status = load_repo_status(&config.git_path, &repo_root);
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
            needs_full_redraw: false,
            repo_root,
            tx,
            rx,
            is_running: false,
            cancel_flag,
            status,
            pending_discard: None,
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
            self.pending_discard = None;
            return Ok(false);
        }

        if let KeyCode::Char('q') = key.code {
            return Ok(true);
        }

        match key.code {
            KeyCode::Char('h') => {
                self.pending_discard = None;
                self.focus = match self.focus {
                    Focus::Cmd => Focus::Cmd,
                    Focus::Files => Focus::Cmd,
                    Focus::Log => Focus::Files,
                    Focus::Result => Focus::Log,
                };
                return Ok(false);
            }
            KeyCode::Char('l') => {
                self.pending_discard = None;
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
                self.pending_discard = None;
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
        let mut selection_changed = false;
        match key.code {
            KeyCode::Char('j') => {
                if self.selected_file + 1 < self.status.files.len() {
                    self.selected_file += 1;
                    selection_changed = true;
                }
            }
            KeyCode::Char('k') => {
                if self.selected_file > 0 {
                    self.selected_file -= 1;
                    selection_changed = true;
                }
            }
            KeyCode::Char('s') => {
                self.pending_discard = None;
                self.toggle_stage_selected_file();
            }
            KeyCode::Char('d') => {
                self.pending_discard = None;
                self.show_diff_for_selected_file(false);
            }
            KeyCode::Char('x') => {
                self.handle_discard_key();
            }
            _ => {
                self.pending_discard = None;
            }
        }

        if selection_changed {
            self.pending_discard = None;
            self.show_diff_for_selected_file(true);
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
        let y = chars.next().unwrap_or(' ');
        let has_unstaged_delete = y == 'D';
        let operands = Self::quoted_operands(entry);

        if operands.is_empty() {
            return;
        }

        let cmd = if is_staged {
            format!("restore --staged -- {}", operands)
        } else {
            if has_unstaged_delete {
                // Deleted in working tree: use -u so git stages the removal even when file is gone.
                format!("add -u -- {}", operands)
            } else {
                format!("add -- {}", operands)
            }
        };
        self.run_command(cmd, LfsMode::None, false);
    }

    fn handle_discard_key(&mut self) {
        if self.status.files.is_empty() {
            self.pending_discard = None;
            return;
        }

        if self.pending_discard == Some(self.selected_file) {
            self.pending_discard = None;
            self.discard_selected_file();
            return;
        }

        self.pending_discard = Some(self.selected_file);
        let entry = &self.status.files[self.selected_file];
        let label = entry.display_label();
        self.result_lines = vec![format!(
            "Discard changes to \"{}\"? (press x again to confirm, any other key cancels)",
            label
        )];
        self.result_scroll = 0;
    }

    fn discard_selected_file(&mut self) {
        if self.status.files.is_empty() {
            return;
        }
        let entry = &self.status.files[self.selected_file];
        let operands = Self::quoted_operands(entry);
        if operands.is_empty() {
            return;
        }

        let cmd = if entry.status == "??" {
            format!("clean -fd -- {}", operands)
        } else {
            format!("restore --staged --worktree -- {}", operands)
        };
        self.run_command(cmd, LfsMode::None, false);
    }

    fn show_diff_for_selected_file(&mut self, is_auto: bool) {
        if self.status.files.is_empty() {
            return;
        }
        if self.is_running {
            if !is_auto {
                self.result_lines
                    .push("WARN: cannot show diff while git is running".into());
            }
            return;
        }

        let entry = &self.status.files[self.selected_file];
        let operands = Self::clean_operands(entry);
        if operands.is_empty() {
            if !is_auto {
                self.result_lines
                    .push("WARN: could not resolve file path for diff".into());
            }
            return;
        }

        let (args, cmd_label) = if entry.status == "??" {
            let dev_null: String = if cfg!(windows) { "NUL".into() } else { "/dev/null".into() };
            let mut args = vec!["diff".into(), "--no-index".into(), "--".into(), dev_null.clone()];
            args.extend(operands.clone());
            let pretty_label = format!(
                "git diff --no-index -- {} {}",
                dev_null,
                operands.join(" ")
            );
            (args, pretty_label)
        } else {
            self.build_diff_command(&operands)
        };

        let output = Command::new(&self.config.git_path)
            .args(&args)
            .current_dir(&self.repo_root)
            .output();

        match output {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let stderr = String::from_utf8_lossy(&o.stderr);

                self.log_lines = if stdout.is_empty() {
                    vec!["<no diff output>".into()]
                } else {
                    stdout.lines().map(|s| s.to_owned()).collect()
                };

                self.result_lines = vec![format!("$ {}", cmd_label)];
                self.result_lines
                    .push(format!("git exit code: {}", o.status.code().unwrap_or(-1)));
                if !stderr.is_empty() {
                    self.result_lines.push("--- git stderr ---".into());
                    self.result_lines
                        .extend(stderr.lines().map(|s| s.to_owned()));
                }
                self.log_scroll = 0;
                self.result_scroll = 0;
            }
            Err(e) => {
                self.log_lines = vec!["<no diff output>".into()];
                self.result_lines = vec![format!("$ {}", cmd_label)];
                self.result_lines
                    .push(format!("ERROR: failed to run git diff: {}", e));
                self.log_scroll = 0;
                self.result_scroll = 0;
            }
        }
    }

    fn build_diff_command(&self, operands: &[String]) -> (Vec<String>, String) {
        let raw = self
            .config
            .files_diff_cmd
            .clone()
            .unwrap_or_else(|| "diff HEAD --".into());
        let mut args = parse_args_line(&raw);
        if !args.is_empty() && args[0] == "git" {
            args.remove(0);
        }
        if args.is_empty() {
            args = vec!["diff".into(), "HEAD".into(), "--".into()];
        }

        let mut final_args = Vec::new();
        let mut inserted_files = false;
        for arg in args {
            if arg == "{files}" {
                final_args.extend_from_slice(operands);
                inserted_files = true;
            } else {
                final_args.push(arg);
            }
        }

        if !inserted_files {
            if !final_args.iter().any(|a| a == "--") {
                final_args.push("--".into());
            }
            final_args.extend_from_slice(operands);
        }

        let label = format!(
            "git {}",
            final_args
                .iter()
                .map(|a| {
                    if a.contains(' ') {
                        format!("\"{}\"", a)
                    } else {
                        a.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        );

        (final_args, label)
    }

    fn clean_operands(entry: &RepoFile) -> Vec<String> {
        entry
            .operands()
            .into_iter()
            .map(|p| p.trim_matches('"').to_string())
            .collect()
    }

    fn quoted_operands(entry: &RepoFile) -> String {
        entry
            .operands()
            .iter()
            .map(|p| format!("\"{}\"", p.trim_matches('"')))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn run_command(&mut self, args_str: String, lfs_mode: LfsMode, interactive: bool) {
        self.pending_discard = None;
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
        let repo_path = self.repo_root.clone();

        thread::spawn(move || {
            let res =
                run_git_with_lfs(git_path, args_str, lfs_mode, cancel_flag.clone(), repo_path);
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
        let repo = self.repo_root.clone();
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
                execute!(stdout, EnterAlternateScreen, Clear(ClearType::All))?;
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
        self.needs_full_redraw = true;
        self.refresh_repo_status();
    }

    fn refresh_repo_status(&mut self) {
        self.status = load_repo_status(&self.config.git_path, &self.repo_root);
        if self.selected_file >= self.status.files.len() && !self.status.files.is_empty() {
            self.selected_file = self.status.files.len() - 1;
        }
        if self.status.files.is_empty() {
            self.selected_file = 0;
        }
        self.pending_discard = None;
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

    pub fn take_full_redraw(&mut self) -> bool {
        let flag = self.needs_full_redraw;
        self.needs_full_redraw = false;
        flag
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
