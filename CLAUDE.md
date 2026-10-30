# amux - Terminal Multiplexer

## Project Overview

A cross-platform terminal multiplexer combining the Ghostty terminal engine (libghostty-vt) with Xilem/Vello GPU-accelerated rendering.

- **Type**: Desktop application (terminal emulator/multiplexer)
- **Language**: Rust
- **Framework**: Xilem + Masonry (canvas) + Vello
- **Version**: 0.5.0

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| `libghostty-vt` | Terminal emulation (ANSI parsing, scrollback, selection, colors) |
| `xilem` / `xilem_masonry` | Declarative view tree, widgets, canvas |
| `portable-pty` | PTY management |
| `parley` / `peniko` | Text layout and color, through masonry |

## Build & Run

```bash
cargo build
cargo run
cargo build --release
cargo test --release        # headless renderer + single-instance tests
```

First build of `libghostty-vt-sys` clones the pinned Ghostty commit and compiles it with Zig (needs network + Zig 0.15.2). Offline/reproducible builds: set `GHOSTTY_SOURCE_DIR` to a Ghostty source checkout (e.g. `vendor/ghostty-a887df42c56f6de86c0fe6da9c4eeca37931e083`, gitignored) and optionally `GHOSTTY_ZIG_SYSTEM_DIR` for prefetched Zig packages. `.cargo/config.toml` already sets `GHOSTTY_SOURCE_DIR` (relative) for this machine; plain `cargo build` works.

Building `libghostty-vt-sys` needs the MSVC environment: run cargo from a Developer Command Prompt (`call vcvars64.bat`), otherwise `cc-rs` fails with "cannot open windows.h".

## Architecture

- `src/main.rs` - `app_logic` (view tree + redraw task), Win32 helpers, single-instance wiring
- `src/app.rs` - Global state: groups → tabs → pane trees; all event handling
- `src/terminal.rs` - Ghostty engine + PTY (reader/writer threads, selection, mouse reports)
- `src/terminal_render.rs` - Render snapshot → scene (the hot path, see below)
- `src/terminal_view.rs` - Canvas widget: pane layout, hover, resize queueing, frame profile
- `src/terminal_keyboard.rs` - Masonry widgets: keyboard capture, focusless buttons
- `src/keyboard.rs` - Direct hotkey table + key→ANSI encoding
- `src/pane_tree.rs`, `src/tab.rs`, `src/group.rs` - Layout and containers
- `src/single_instance.rs` - Discovery file + path forwarding
- `src/config.rs` - TOML config (`%APPDATA%/amux/config.toml`)

## Keyboard Shortcuts

`Ctrl+B` is the tmux-style prefix; the next key is a multiplexer command.

| Shortcut | Action |
|----------|--------|
| Ctrl+B, c / % | Split pane down |
| Ctrl+B, d / " | Split pane right |
| Ctrl+B, n / p | Focus next / previous pane |
| Ctrl+B, x | Close pane |
| Ctrl+B, t / w | New tab / close tab |
| Ctrl+B, b | Toggle sidebar |
| Ctrl+T / Ctrl+W | New tab / close tab |
| Ctrl+Tab / Ctrl+Shift+Tab | Next / previous tab |
| Ctrl+D / Ctrl+Shift+D | Split right / split down |
| Ctrl+Alt+arrows | Focus pane directionally |
| Ctrl+K | Back to live prompt |
| Ctrl+N | New workspace |
| Ctrl+= / Ctrl+- / Ctrl+0 | Font size +1 / −1 / reset |
| Ctrl+, / Ctrl+Shift+, | Settings / open config dir |
| Shift+PgUp/PgDn | Scroll back one page |

The full table lives in README.md.

## Rendering pipeline (read before touching performance)

`src/terminal_render.rs::render_grid` is the hot path. A repaint re-records the
whole scene, so the rules that keep frames cheap are:

1. **Only repaint when something visible changed.** `TerminalKeyboardView::message`
   returns `MessageResult::Nop` unless its handler changed the picture; the PTY
   wake-ups coalesce through `needs_redraw`; the OSC title callback only wakes on
   an actual change.
2. **One FFI call per fact.** Every accessor on a `Cell` / `CellIteration` crosses
   the FFI boundary. Cell text goes through `graphemes_utf8` into a reused buffer
   (the C API *replaces* the buffer, it does not append, and answers an empty
   string for cells without text, so no `has_text` query is needed); `raw_cell()`
   is fetched once for the `wide` flag; cells with neither text nor decoration
   skip the foreground lookup.
3. **Merge fills.** Adjacent same-colour backgrounds / underlines become one rect
   (`FillRun`).
4. **Batch glyph runs.** `GLYPH_LAYOUTS` caches a `GlyphEntry` per distinct
   text: the parley layout, the glyph ids with their cell-relative offsets, and
   the font identity (blob id, size, variable axes) a run must match. Cells
   sharing font + colour share one `GlyphRun` draw (`batch_entry`); entries
   marked `per_cell` (several runs, a synthesised skew, nothing drawable) and
   cell fillers needing a per-cell vertical stretch stay on `render_text`.
5. **Cache what does not change.** Glyph entries by `font_size` + text, cell
   metrics by `font_size` (`cell_metrics_full`, shared with the view), FxHash-style
   key (`glyph_key`), and overlay labels by text + size + weight (`ui_layout`,
   which also serves the measuring pass). All three caches are dropped when the
   bundled font registers, since that can change which face resolves.

`src/terminal_render.rs` has headless tests that render a terminal into a `Scene`
without a window — including one that compares the batched output against a
per-cell `render_text` reference. Extend them when changing the renderer.

`AMUX_FRAME_LOG=1` prints a per-120-frame breakdown: total, snapshot, cell scan,
glyph pass, and cells/glyphs/runs.

## Notes

- Pane layout is a recursive split tree; `Split` carries a `ratio` (0.5 on
  split, draggable: `PaneTree::splits` exposes the handle geometry for the
  pointer, `set_ratio` applies the drag)
- The Ghostty engine is `!Send`: the UI thread owns it via `RefCell`; PTY reader threads only push byte chunks over a channel which the UI thread feeds via `vt_write`
- On Windows a broken ConPTY read is not treated as shell exit; only `child.wait()` flags it, and an abnormal exit prints a notice instead of closing the pane
- Config (font size, window geometry, single-instance) is saved on exit
