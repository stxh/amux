# amux - Terminal Multiplexer

## Project Overview

A cross-platform terminal multiplexer combining Alacritty terminal backend with Xilem/Vello GPU-accelerated rendering.

- **Type**: Desktop application (terminal emulator/multiplexer)
- **Language**: Rust
- **Framework**: Xilem + Vello

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| `alacritty_terminal` | Terminal emulation (PTY, ANSI, scrollback) |
| `xilem` | UI framework |
| `portable-pty` | PTY management |

## Build & Run

```bash
cargo build
cargo run
cargo build --release
```

## Architecture

- `src/main.rs` - Entry point, app initialization
- `src/app.rs` - Global state (panes, tabs)
- `src/terminal.rs` - Terminal wrapper with PTY I/O
- `src/terminal_render.rs` / `terminal_view.rs` - Rendering
- `src/keyboard.rs` - Input handling (Ctrl+B prefix)
- `src/pane_tree.rs`, `tab.rs` - Layout management
- `src/config.rs` - Configuration (TOML)

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| Ctrl+B, c | Split vertically |
| Ctrl+B, d / Ctrl+B, " | Split horizontally |
| Ctrl+B, n | Next pane |
| Ctrl+B, p | Previous pane |
| Ctrl+B, x | Close pane |
| Shift+PgUp/PgDn | Scroll |

## Notes

- Pane layout uses normalized coordinates for responsive sizing
- PTY output read in background threads, fed to alacritty_terminal parser
- Status bar shows available shortcuts