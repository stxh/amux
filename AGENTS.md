# amux - Terminal Multiplexer

## Project Overview

A cross-platform terminal multiplexer combining the Ghostty terminal engine (libghostty-vt) with Xilem/Vello GPU-accelerated rendering.

- **Type**: Desktop application (terminal emulator/multiplexer)
- **Language**: Rust
- **Framework**: Xilem + Vello

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| `libghostty-vt` | Terminal emulation (ANSI parsing, scrollback, selection, colors) |
| `xilem` | UI framework |
| `portable-pty` | PTY management |

## Build & Run

```bash
cargo build
cargo run
cargo build --release
```

First build of `libghostty-vt-sys` clones the pinned Ghostty commit and compiles it with Zig (needs network + Zig 0.15.2). Offline/reproducible builds: set `GHOSTTY_SOURCE_DIR` to a Ghostty source checkout (e.g. `vendor/ghostty-a887df42c56f6de86c0fe6da9c4eeca37931e083`, gitignored) and optionally `GHOSTTY_ZIG_SYSTEM_DIR` for prefetched Zig packages. `.cargo/config.toml` already sets `GHOSTTY_SOURCE_DIR` (relative) for this machine; plain `cargo build` works.

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
| Ctrl+B, b | Toggle sidebar |
| Shift+PgUp/PgDn | Scroll |

## Notes

- Pane layout uses normalized coordinates for responsive sizing
- The Ghostty engine is `!Send`: the UI thread owns it via `RefCell`; PTY reader threads only push byte chunks over a channel which the UI thread feeds via `vt_write`
- Status bar shows available shortcuts