# amux - Terminal Multiplexer

A cross-platform terminal multiplexer combining Alacritty terminal backend with Xilem/Vello GPU-accelerated rendering.

## Overview

amux is a terminal multiplexer inspired by tmux and cmux, featuring:
- **Multi-pane layout** - Split terminals vertically and horizontally
- **GPU-accelerated rendering** - Uses Vello imaging backend for fast rendering
- **PTY integration** - Each pane runs its own shell process
- **ANSI color support** - Full 256-color palette with proper terminal emulation
- **Cross-platform** - Works on Windows, macOS, and Linux

## Architecture

```
+---------------------------------------------------+
|  [Terminal Canvas - Vello GPU rendering]          |
|  +-------------------+  +---------------------+   |
|  | Pane 0 (active)   |  | Pane 1              |   |
|  | [shell + PTY]     |  | [shell + PTY]       |   |
|  +-------------------+  +---------------------+   |
+---------------------------------------------------+
| Ctrl+B: prefix | c:split-v | %:split-h | ...      |
+---------------------------------------------------+
```

## Technology Stack

- **Rust** - Systems programming language for performance and safety
- **Xilem** - Declarative UI framework with canvas-based rendering
- **Vello** - GPU-accelerated 2D graphics rendering
- **alacritty_terminal** - Terminal emulation backend (PTY, ANSI parsing, scroll buffer)
- **portable-pty** - Cross-platform pseudo-terminal management

## Features

### Terminal Rendering
- Multi-pane rendering with proper layout geometry using normalized coordinates
- GPU-accelerated rendering via Vello imaging backend
- ANSI color support with full 256-color palette
- Proper TermColor to peniko Color conversion
- Cell flag rendering: Bold, Underline, Strikethrough, Inverse
- Cursor block rendering for active pane
- Scroll indicators for scrollback history
- Active/inactive pane borders (blue for active, surface for inactive)

### Pane Management
- Split panes vertically (top/bottom)
- Split panes horizontally (side by side)
- Navigate between panes (next/previous)
- Close panes
- Layout uses normalized coordinates for responsive resizing

### PTY Integration
- Each pane has its own shell process
- Background threads read PTY output asynchronously
- Output fed into alacritty_terminal parser
- Proper event handling for terminal state changes

### UI
- Catppuccin Mocha color scheme for beautiful dark theme
- Status bar showing keyboard shortcuts
- Button-based actions for mouse interaction
- Terminal canvas fills the main window area (cmux-style layout)

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| Ctrl+B, c | Split vertically |
| Ctrl+B, d or Ctrl+B, " | Split horizontally |
| Ctrl+B, n | Next pane |
| Ctrl+B, p | Previous pane |
| Ctrl+B, x | Close pane |
| Shift+PgUp | Scroll up |
| Shift+PgDn | Scroll down |

## Project Structure

```
amux/
├── Cargo.toml          # Project dependencies
├── src/
│   ├── main.rs         # Application entry point and UI layout
│   ├── app.rs          # Global application state management
│   ├── pane.rs         # Pane representation and layout geometry
│   ├── terminal.rs     # Alacritty terminal wrapper with PTY I/O
│   ├── terminal_render.rs  # Canvas-based terminal rendering
│   ├── terminal_widget.rs  # Custom Masonry widget (for future use)
│   └── keyboard.rs     # Keyboard input handling module
└── README.md           # This file
```

## Implementation Details

### Canvas-Based Rendering

The application uses Xilem's canvas pattern for terminal rendering:
- Terminal data stays in App state (no need to move/clone terminals)
- View is pure rendering - canvas callback just draws what's in App state
- State is preserved through TerminalPaneState (tracks scroll/focus per pane)
- Simple and direct approach - no complex widget integration needed

### Terminal State Management

- **App**: Manages vector of panes and their states
- **Pane**: Contains Terminal and layout information
- **Terminal**: Wraps alacritty_terminal's Term with PTY I/O
- **TerminalPaneState**: UI-specific state (scroll offset, focus)

### PTY Data Flow

1. Pane spawns shell process via portable-pty
2. Background thread reads from PTY master
3. Output fed into alacritty_terminal's input parser
4. Terminal state updated asynchronously
5. Canvas re-renders on next frame with updated terminal grid

### Color Handling

Full 256-color palette support:
- Named colors (16 standard ANSI colors)
- 216-color cube (indices 16-231)
- 24 grayscale (indices 232-255)
- True color (24-bit RGB)

All colors converted from alacritty's TermColor to peniko's Color type.

## Building and Running

```bash
# Build
cargo build

# Run
cargo run

# Release build
cargo build --release
```

## Dependencies

- `alacritty_terminal` (0.24.0) - Terminal emulation
- `xilem` (git) - UI framework
- `portable-pty` (0.8) - PTY management
- `parley` (0.8) - Text layout (for future use)
- `skrifa` (0.40) - Font handling (for future use)
- `peniko` (0.6) - Color handling
- `log` / `env_logger` - Logging

## Future Enhancements

- Real glyph rendering using parley/skrifa
- Mouse-based text selection
- Copy/paste support
- Scrollback buffer navigation
- Pane zoom (focus on single pane)
- Session persistence
- Configuration file support
- Status bar customization
- Font size adjustment

## License

MIT

## Credits

Inspired by:
- **tmux** - Terminal multiplexer with prefix-key commands
- **cmux** - Modern terminal multiplexer with clean UI
- **Alacritty** - Fast GPU-accelerated terminal emulator

Built with:
- **Xilem** - Declarative UI framework by Linebender
- **alacritty_terminal** - Terminal backend by Alacritty contributors
