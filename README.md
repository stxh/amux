# amux — Terminal Multiplexer

A cross-platform terminal multiplexer that combines the **Ghostty terminal engine**
(`libghostty-vt`) with **Xilem / Masonry / Vello** GPU-accelerated rendering.

Panes are drawn straight from the engine's render snapshot onto a single masonry
canvas, so a split workspace is one retained-mode scene rather than one widget per
cell, pane or tab.

```
┌────────┬─────────────────────────────────────────────────────────┬──────┐
│ ⚙️     │ [ tab1 ] [ tab2 ] [ + ]                                │      │
│ 💻  ←  ├─────────────────────────────────────────────────────────┤ cpu  │
│ 🚀     │ ┌──────────────────────────┐┌────────────────────────┐  │ mem  │
│ 🌐     │ │ pane: zsh (curses app)  ││ pane: tail -f log      │  │      │
│ 🛠️  ←  │ └──────────────────────────┘└────────────────────────┘  │      │
│ 📊     │                          ↑ draggable scrollbar          │      │
│ ➕     │                                                        │      │
└────────┴─────────────────────────────────────────────────────────┴──────┘
```

The right strip is a placeholder panel (`Ctrl+Alt+B` toggles it) waiting for
real metrics.

## Features

**Multiplexing**
- Workspaces (groups) with auto-assigned emoji icons in the left activity bar
- Hover a workspace icon for a tooltip with its name, terminal title and the
  shell's *live* working directory (read from the shell process, so `cd` shows
  up without shell integration; PowerShell prompts and OSC 7 reports are read
  as well, since `Set-Location` never moves the process directory); the tooltip
  hides itself after 5 s
- Right-click a workspace icon to close it after a confirmation; the last
  workspace is never closed
- Tabs per workspace, nested split panes per tab; a new tab or pane starts in
  the directory the workspace's most recent terminal is in
- Drag a split divider to resize the panes on either side; each split keeps its
  own ratio
- Per-pane shells: each pane owns its own PTY and its own terminal engine
- Click a pane to focus it; when a pane is split, a close button appears over
  the one under the pointer

**Terminal**
- Ghostty VT engine: 256-colour palette + truecolour, underline/strikethrough,
  wide (CJK) cells, grapheme clusters, up to 10 000 lines of scrollback
- Mouse reporting (DEC 1000/1002/1003) forwarded to the hosted app; local
  word/line selection and copy-on-select otherwise (hold Shift to override)
- Draggable scrollbar with paging, wheel scroll, and edge auto-scroll while
  selecting past the top/bottom row
- Mouse is captured at the OS level, so a drag keeps tracking outside the window
- Box-drawing characters are stretched to the cell height, so borders stay
  contiguous even when CJK fonts make rows taller
- Bundled Symbols Nerd Font Mono so powerline/Nerd icons render
- Desktop notifications for OSC 9 / 777 / 99 notifications

**App**
- `amux <path>` opens a shell in that directory; with `single_instance = true`
  a second `amux` forwards its path to the running window as a new tab and exits
- Settings overlay (gear, or `Ctrl+,`): font size, sidebars, single-instance mode
- Window size, position and font size are persisted on exit
- Catppuccin Mocha theme, rounded DWM window corners on Windows 11

## Keyboard shortcuts

`Ctrl+B` enters prefix mode; the next key is a multiplexer command.

| Shortcut | Action |
|---|---|
| `Ctrl+B`, `c` / `%` | Split pane down |
| `Ctrl+B`, `d` / `"` | Split pane right |
| `Ctrl+B`, `n` / `p` | Focus next / previous pane |
| `Ctrl+B`, `x` | Close pane |
| `Ctrl+B`, `t` / `w` | New tab / close tab |
| `Ctrl+B`, `b` | Toggle left sidebar |
| `Ctrl+B`, `g` | Next group |
| `Ctrl+B`, `1`–`9` | Jump to tab |

While the settings overlay is open it is modal: it swallows every key, and any
key closes it.

Direct hotkeys (no prefix):

| Shortcut | Action |
|---|---|
| `Ctrl+N` | New workspace (group) |
| `Ctrl+1`–`Ctrl+9` | Switch workspace (`Ctrl+9` = last) |
| `Ctrl+Shift+W` / `Ctrl+Shift+R` | Close workspace / rename workspace |
| `Ctrl+T` | New tab |
| `Ctrl+W` | Close tab |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Next / previous tab |
| `Alt+1`–`Alt+9` | Switch tab (`Alt+9` = last) |
| `Ctrl+D` / `Ctrl+Shift+D` | Split right / split down |
| `Ctrl+Alt+←↑→↓` | Focus pane directionally |
| `Ctrl+K` | Scroll back to the live prompt |
| `Ctrl+Alt+B` | Toggle right (monitor) sidebar |
| `Ctrl+,` / `Ctrl+Shift+,` | Settings / open config dir |
| `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | Font size +1 / −1 / reset |
| `Shift+PgUp` / `Shift+PgDn` | Scroll one page |
| any key while settings is open | Close the settings overlay |
| `Esc` / `Enter` in the close-workspace dialog | Cancel / confirm the close |

## Configuration

`%APPDATA%/amux/config.toml` on Windows
(`~/.config/amux/config.toml` elsewhere). Created with defaults on first run.

```toml
font_size = 18.0

[window]
width = 1200.0
height = 800.0
# position = { x = 100.0, y = 80.0 }
maximized = false

single_instance = false
```

## Architecture

```
src/
  main.rs              app_logic: view tree, the redraw task, Win32 helpers
  app.rs               global state: groups → tabs → pane trees, event handling
  terminal.rs          Ghostty engine + PTY: reader/writer threads, selection
  terminal_render.rs   snapshot → cells → retained scene (the hot path)
  terminal_view.rs     canvas widget: pane layout, hover, resize queueing
  terminal_keyboard.rs masonry widgets: keyboard capture, focusless buttons
  sidebar_icon.rs      activity-bar icon widget: click / right-click / hover
  keyboard.rs          hotkey table + key → ANSI encoding
  pane_tree.rs         recursive split tree, layout and draggable splitters
  pane.rs              one pane: a terminal plus its active flag
  tab.rs / group.rs    tab and workspace containers
  single_instance.rs   named-mutex discovery + path forwarding
  proc_cwd.rs          live cwd of the shell process (PEB read on Windows)
  config.rs            TOML config
```

### Data flow

1. A PTY reader thread reads bytes, sniffs OSC notifications, and pushes raw
   chunks over a channel — coalescing the redraw wake-up to at most one in
   flight.
2. The UI thread drains the channel into the engine (`vt_write`) and bumps
   `redraw_counter`.
3. Xilem re-runs `app_logic`; the canvas closure re-records the scene: pane
   backgrounds → per-row cell scan → merged fill runs → batched glyph runs →
   cursor and scrollbar.
4. Masonry replays the scene through Vello; the driver presents the frame.

The engine is `!Send`, so it is owned by the UI thread in a `RefCell`. Reader
threads never touch it.

## Rendering and performance

Terminal output goes to the GPU through four mechanisms that keep per-frame
work proportional to what actually changed:

**1. Nothing to redraw, nothing to rebuild.** A repaint re-records the whole
scene, so events only ask for one when they changed something visible. Pointer
moves report whether they did — hover only paints on the settings overlay and
split-pane close buttons, and a selection drag only repaints when it crosses
into a new cell. Unchanged OSC titles, duplicate PTY wake-ups and the idle
auto-scroll timer are dropped before they reach the view.

**2. Allocation-free cell scan.** A grid holds thousands of cells and every
cell accessor is an FFI call, so each cell is asked exactly what it needs: one
`raw_cell` fetch serves the wide flag, `graphemes_utf8` doubles as the "does
this cell have text" query (an empty answer means it does not), cell texts are
packed back to back into one per-row arena, and cells with neither text nor
decoration never resolve a foreground colour. Plain spaces skip the glyph path
entirely.

**3. Merged fill runs.** Adjacent cells sharing a background colour — or a
underline colour — are drawn as one rect, so a full-width colour band is one
draw op instead of eighty.

**4. Batched glyph runs.** Everything that depends only on a cell's text is
cached by font size and text: the parley layout, the glyph ids with their
offsets inside the cell, and the font identity a run has to match. Neighbouring
cells that resolve to the same font and colour are then emitted as a single
glyph run — a 120×40 text screen goes from ~2 300 draw ops and ~2 300 heap
allocations per frame to a few hundred. Cell fillers that need a per-cell
vertical stretch still take the one-cell path. Overlay labels get the same
treatment, so hovering the settings panel no longer rebuilds its layouts.

Measured with the two builds alternating on one 120×40 pane (release, scene
recorded, best of five runs each — a desktop machine is noisy, and the old
path's per-cell allocations make it much more load-sensitive than the new one,
so the ratio moves a point or two between runs):

| screen | before | after | speedup |
|---|---|---|---|
| mixed text + colours | 4.60 ms | 1.33 ms | 3.5× |
| full-width background bands | 4.70 ms | 1.00 ms | 4.7× |
| prompt with a few lines | 1.40 ms | 0.60 ms | 2.3× |

Idle cost is the other half of the story and is not in this table: before,
moving the mouse over the window re-recorded the whole scene on every pointer
event, so a still terminal still burned a frame per move.

### Measuring

```bash
AMUX_FRAME_LOG=1 cargo run --release
```

Reports every 120 frames, split by phase:

```
[amux frame] n=120 avg=1.33ms max=2.11ms | snapshot=0.01ms scan=0.38ms \
  glyphs=0.52ms | 4680 cells 2267 glyphs in 224 runs
```

`runs` is the number of glyph-run draw ops — the batching ratio is
`glyphs / runs`.

## Building

```bash
cargo build
cargo run
cargo build --release
cargo test --release          # headless renderer + single-instance tests
```

The first build of `libghostty-vt-sys` clones the pinned Ghostty commit and
compiles it with **Zig 0.15.2**. For offline/reproducible builds, point
`GHOSTTY_SOURCE_DIR` at a Ghostty checkout (this repo keeps one under
`vendor/`, gitignored) and optionally `GHOSTTY_ZIG_SYSTEM_DIR` at prefetched
Zig packages. `.cargo/config.toml` already sets `GHOSTTY_SOURCE_DIR` for this
machine.

Compiling the engine's C shim needs the MSVC environment, so run cargo from a
Developer Command Prompt (`vcvars64.bat`); otherwise `cc-rs` fails looking for
`windows.h`.

## Credits

Built with:

- **[libghostty-vt]** — the terminal engine behind the grid, selection and
  scrollback
- **[Xilem]** / **[Masonry]** / **[Vello]** — UI framework, retained-mode
  rendering and GPU rasterisation
- **[portable-pty]**, **[parley]**, **[peniko]**

Inspired by **tmux** (prefix-key multiplexing), **cmux** (workspace-first UI) and
**Alacritty** (renderer design).

[libghostty-vt]: https://crates.io/crates/libghostty-vt
[Xilem]: https://github.com/linebender/xilem
[Masonry]: https://github.com/linebender/xilem/tree/main/masonry
[Vello]: https://github.com/linebender/vello
[portable-pty]: https://crates.io/crates/portable-pty
[parley]: https://crates.io/crates/parley
[peniko]: https://crates.io/crates/peniko