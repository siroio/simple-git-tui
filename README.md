# simple-git-tui

A lightweight, vim-oriented Git TUI written in Rust.

This tool is designed for developers who want a **fast, keyboard-driven Git workflow** with:
- Git status / graph / branch inspection
- Git LFS-aware fetch & pull
- Asynchronous execution (UI never blocks)
- Vim-style keybindings
- Simple TOML-based configuration

> Tested on Windows with Git for Windows.

---

## Features

- ✅ Vim keybindings (`hjkl`, `j/k`, `Ctrl+u/d`, `:`)
- ✅ Git graph with color (`--decorate`, `--graph`)
- ✅ Git LFS support (fetch / pull with real data download)
- ✅ Non-blocking async command execution
- ✅ Cancel running command (`Ctrl+C`)
- ✅ ANSI color rendering in TUI
- ✅ Configurable commands via `config.toml`

---

## Screenshot

_(Add a screenshot here once you are ready)_

---

## Installation

### Requirements

- Rust (latest stable)
- Git (Git for Windows recommended)
- Git LFS (optional, if using LFS repos)

### Build

```bash
git clone https://github.com/yourname/simple-git-tui.git
cd simple-git-tui
cargo build --release
```

Binary will be created at:

```
target/release/simple-git-tui.exe
```

---

## Usage

```bash
cargo run
# or
./simple-git-tui.exe
```

Controls:

| Key | Action |
|---|---|
| j / k | Move selection |
| h / l | Change focus pane |
| Enter | Execute selected command |
| Ctrl+u / Ctrl+d | Scroll |
| PgUp / PgDn | Page scroll |
| : | Command line mode |
| Ctrl+C | Cancel running command |
| q | Quit |

---

## Configuration (`config.toml`)

Place `config.toml` next to the executable.

Example:

```toml
git_path = "C:\\Program Files\\Git\\bin\\git.exe"
repo_path = "D:\\YourRepo"

[[commands]]
name = "Status"
cmd  = "status -sb --color=always"

[[commands]]
name = "Graph"
cmd  = "log --oneline --graph --decorate --all --color=always"

[[commands]]
name = "Fetch + LFS"
cmd  = "fetch --all --prune"
lfs  = "fetch"

[[commands]]
name = "Pull + LFS"
cmd  = "pull"
lfs  = "pull"
```

---

## Architecture

```text
src/
 ├─ main.rs     # entry point
 ├─ app.rs      # TUI + event loop
 ├─ git.rs      # git / lfs execution
 ├─ config.rs   # TOML config
 └─ theme.rs    # color theme
```

---

## License

MIT License.

You are free to use, modify, and redistribute this software at your own risk.
No warranty is provided.

---

## Disclaimer

This tool executes `git` commands directly.
Always ensure you understand what commands are defined in `config.toml`
before running them on important repositories.
