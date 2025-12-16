pub const DEFAULT_CONFIG: &str = r#"
git_path = "git"

[colors]
accent = "cyan"
error = "red"
background = "black"

[layout]
cmd_width = 32
files_height = 7
result_height = 5

files_diff_cmd = "diff HEAD --"

[[commands]]
name = "Status"
cmd  = "status -sb"

[[commands]]
name = "Graph"
cmd  = "log --oneline --graph --decorate --all --color=always"

[[commands]]
name = "Pull"
cmd  = "pull"
lfs  = "pull"

[[commands]]
name = "Push"
cmd = "push"
lfs = "push"

[[commands]]
name = "Commit"
cmd = "commit"
interactive = true
"#;
