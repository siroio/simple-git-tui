use std::env;
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone, Copy)]
pub enum LfsMode {
    None,
    Fetch,
    Pull,
}

pub fn parse_lfs_mode(opt: Option<&String>) -> LfsMode {
    match opt.map(|s| s.as_str()) {
        Some("fetch") => LfsMode::Fetch,
        Some("pull") => LfsMode::Pull,
        _ => LfsMode::None,
    }
}


fn parse_args_line(s: &str) -> Vec<String> {
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
