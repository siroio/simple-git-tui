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

[[commands]]
name = "Status"
cmd  = "status -sb --color=always"

[[commands]]
name = "Graph"
cmd  = "log --oneline --graph --decorate --all --color=always"

[[commands]]
name = "Fetch"
cmd  = "fetch --all --prune"

[[commands]]
name = "Pull + LFS"
cmd  = "pull"
lfs  = "pull"
"#;

