# simple-git-tui

A fast, vim-oriented Git TUI written in Rust.

**simple-git-tui** is a lightweight, keyboard-driven Git interface inspired by  
**Vim**, **gitui**, and other terminal-native workflows.

It is designed for developers who want:
- zero mouse usage
- non-blocking Git operations
- fast inspection and staging
- minimal configuration

> Tested on Windows (Git for Windows).  
> Linux / macOS should work as well.

---

## Features

- ✅ Vim-style keybindings (`hjkl`, `j/k`, `Ctrl+u/d`, `:`)
- ✅ Multi-pane TUI (Commands / Files / Log / Result)
- ✅ Git status, graph, branches
- ✅ Per-file stage / unstage UI
- ✅ Git LFS-aware fetch & pull
- ✅ Fully asynchronous execution (UI never blocks)
- ✅ Cancel running commands (`Ctrl+C`)
- ✅ ANSI color rendering inside TUI
- ✅ Auto-generated TOML configuration
- ✅ Works by launching **inside a Git repository**

---

## Installation

### Requirements

- Rust (latest stable)
- Git (Git for Windows recommended)
- Git LFS (optional)

### Build

```bash
git clone https://github.com/siroio/simple-git-tui.git
cd simple-git-tui
cargo build --release
```

---

## Usage

```bash
cd path/to/your/repo
simple-git-tui
```

---

## Configuration

Config file is auto-generated on first launch.

### Location

- Windows: `%LOCALAPPDATA%\simple-git-tui\config.toml`
- Linux: `~/.config/simple-git-tui/config.toml`
- macOS: `~/Library/Application Support/simple-git-tui/config.toml`

---

## License

MIT License.
