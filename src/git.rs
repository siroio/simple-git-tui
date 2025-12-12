use std::env;
use std::path::Path;
use std::process::Command;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy)]
pub enum LfsMode {
    None,
    Fetch,
    Pull,
}

#[derive(Clone, Debug)]
pub struct RepoFile {
    pub status: String,
    pub path: String,
}

impl RepoFile {
    pub fn operands(&self) -> Vec<String> {
        let parts: Vec<&str> = self.path.split(" -> ").collect();
        if parts.len() == 2 {
            return parts.into_iter().map(|s| s.to_string()).collect();
        }
        vec![self.path.clone()]
    }

    pub fn display_label(&self) -> String {
        let path_ref = self.path.as_str();
        let parts: Vec<&str> = path_ref.split(" -> ").collect();
        let target = parts.last().copied().unwrap_or(path_ref);
        let cleaned = target.trim_matches('"');
        let base = cleaned
            .rsplit(|c| c == '/' || c == '\\')
            .find(|s| !s.is_empty())
            .unwrap_or(cleaned);

        if base.is_empty() {
            target.to_string()
        } else {
            base.to_string()
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RepoStatus {
    pub branch: String,
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    pub files: Vec<RepoFile>,
}

impl RepoStatus {
    pub fn summary(&self) -> String {
        format!(
            "[{}] +{} ~{} ?{}",
            self.branch, self.staged, self.unstaged, self.untracked
        )
    }
}

pub fn parse_lfs_mode(opt: Option<&String>) -> LfsMode {
    match opt.map(|s| s.as_str()) {
        Some("fetch") => LfsMode::Fetch,
        Some("pull") => LfsMode::Pull,
        _ => LfsMode::None,
    }
}

pub fn load_repo_status(git: &str, repo: &Path) -> RepoStatus {
    let branch = Command::new(git)
        .arg("rev-parse")
        .arg("--abbrev-ref")
        .arg("HEAD")
        .current_dir(repo)
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

    let mut status = RepoStatus {
        branch,
        ..RepoStatus::default()
    };

    let output = Command::new(git)
        .arg("status")
        .arg("--porcelain=v1")
        .current_dir(repo)
        .output();

    if let Ok(o) = output {
        if o.status.success() {
            let text = String::from_utf8_lossy(&o.stdout);
            for line in text.lines() {
                if line.len() < 3 {
                    continue;
                }
                let file_status = line[..2].to_string();
                let raw_path = line[3..].to_string();

                if file_status == "??" {
                    status.untracked += 1;
                } else {
                    let mut chars = file_status.chars();
                    let x = chars.next().unwrap_or(' ');
                    let y = chars.next().unwrap_or(' ');
                    if x != ' ' {
                        status.staged += 1;
                    }
                    if y != ' ' {
                        status.unstaged += 1;
                    }
                }

                status.files.push(RepoFile {
                    status: file_status,
                    path: raw_path,
                });
            }
        }
    }

    status
}

pub fn parse_args_line(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut escape = false;

    for c in s.chars() {
        if escape {
            current.push(c);
            escape = false;
        } else if c == '\\' && in_quotes {
            escape = true;
        } else if c == '"' {
            in_quotes = !in_quotes;
        } else if c.is_whitespace() && !in_quotes {
            if !current.is_empty() {
                args.push(current.clone());
                current.clear();
            }
        } else {
            current.push(c);
        }
    }

    if !current.is_empty() {
        args.push(current);
    }

    args
}

pub struct CommandResult {
    pub log_lines: Vec<String>,
    pub result_lines: Vec<String>,
}

pub fn run_git_with_lfs(
    git_path: String,
    args_str: String,
    lfs_mode: LfsMode,
    cancel_flag: Arc<AtomicBool>,
) -> CommandResult {
    let mut log_lines = Vec::new();
    let mut result_lines = Vec::new();

    result_lines.push(format!("$ git {}", args_str));

    let repo_path = env::current_dir().unwrap_or_else(|_| ".".into());

    let mut parts = parse_args_line(&args_str);

    if parts.is_empty() {
        result_lines.push("ERROR: empty git command".into());
        return CommandResult {
            log_lines,
            result_lines,
        };
    }

    let subcmd = parts.remove(0);

    let main_output = Command::new(&git_path)
        .arg(&subcmd)
        .args(&parts)
        .current_dir(&repo_path)
        .output();

    match main_output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            if stdout.is_empty() {
                log_lines.push("<no stdout from git>".into());
            } else {
                log_lines.extend(stdout.lines().map(|s| s.to_owned()));
            }

            result_lines.push(format!(
                "git exit code: {}",
                output.status.code().unwrap_or(-1)
            ));
            if !stderr.is_empty() {
                result_lines.push("--- git stderr ---".into());
                result_lines.extend(stderr.lines().map(|s| s.to_owned()));
            }
        }
        Err(e) => {
            result_lines.push(format!("ERROR: failed to run git: {}", e));
            return CommandResult {
                log_lines,
                result_lines,
            };
        }
    }

    if cancel_flag.load(Ordering::Relaxed) {
        result_lines.push("<canceled before LFS stage>".into());
        return CommandResult {
            log_lines,
            result_lines,
        };
    }

    match lfs_mode {
        LfsMode::None => {}
        LfsMode::Fetch => {
            result_lines.push(String::new());
            result_lines.push("== git lfs fetch --all ==".into());

            let lfs_output = Command::new(&git_path)
                .arg("lfs")
                .arg("fetch")
                .arg("--all")
                .current_dir(&repo_path)
                .output();

            match lfs_output {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                    if !stdout.is_empty() {
                        log_lines.push(String::new());
                        log_lines.push("--- git lfs fetch --all ---".into());
                        log_lines.extend(stdout.lines().map(|s| s.to_owned()));
                    }

                    result_lines.push(format!(
                        "git lfs fetch exit code: {}",
                        output.status.code().unwrap_or(-1)
                    ));
                    if !stderr.is_empty() {
                        result_lines.push("--- git lfs stderr ---".into());
                        result_lines.extend(stderr.lines().map(|s| s.to_owned()));
                    }
                }
                Err(e) => {
                    result_lines.push(format!("ERROR: failed to run git lfs fetch: {}", e));
                }
            }
        }
        LfsMode::Pull => {
            result_lines.push(String::new());
            result_lines.push("== git lfs pull ==".into());

            let lfs_output = Command::new(&git_path)
                .arg("lfs")
                .arg("pull")
                .current_dir(&repo_path)
                .output();

            match lfs_output {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                    if !stdout.is_empty() {
                        log_lines.push(String::new());
                        log_lines.push("--- git lfs pull ---".into());
                        log_lines.extend(stdout.lines().map(|s| s.to_owned()));
                    }

                    result_lines.push(format!(
                        "git lfs pull exit code: {}",
                        output.status.code().unwrap_or(-1)
                    ));
                    if !stderr.is_empty() {
                        result_lines.push("--- git lfs stderr ---".into());
                        result_lines.extend(stderr.lines().map(|s| s.to_owned()));
                    }
                }
                Err(e) => {
                    result_lines.push(format!("ERROR: failed to run git lfs pull: {}", e));
                }
            }
        }
    }

    CommandResult {
        log_lines,
        result_lines,
    }
}
