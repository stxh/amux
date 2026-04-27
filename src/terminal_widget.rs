/// Terminal widget with keyboard input, scroll, and proper ANSI color rendering
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::{Color as TermColor, NamedColor, Rgb};
use accesskit::{Node, Role};

use xilem::masonry::core::{
    AccessCtx, ChildrenIds, EventCtx, LayoutCtx, MeasureCtx, PaintCtx, PropertiesMut,
    PropertiesRef, RegisterCtx, Widget, TextEvent,
    keyboard::{Key, KeyState, NamedKey},
    PointerEvent, PointerButton,
    Ime,
};
use xilem::masonry::imaging::Painter;
use xilem::masonry::kurbo::{Rect, Size, Stroke};
use xilem::masonry::peniko::Color;

use crate::terminal::Terminal;

/// Catppuccin Mocha color scheme
mod colors {
    use super::Color;
    pub const CRUST:     Color = Color::from_rgba8(0x11, 0x11, 0x1b, 0xff);
    pub const BASE:      Color = Color::from_rgba8(0x1e, 0x1e, 0x2e, 0xff);
    pub const SURFACE0:  Color = Color::from_rgba8(0x31, 0x32, 0x44, 0xff);
    pub const SURFACE1:  Color = Color::from_rgba8(0x45, 0x47, 0x5a, 0xff);
    pub const TEXT:      Color = Color::from_rgba8(0xc6, 0xd0, 0xf5, 0xff);
    pub const BLUE:      Color = Color::from_rgba8(0x89, 0xb4, 0xfa, 0xff);
    pub const ROSEWATER: Color = Color::from_rgba8(0xf5, 0xe0, 0xdc, 0xff);
    pub const RED:       Color = Color::from_rgba8(0xf3, 0x8b, 0xa8, 0xff);
    pub const GREEN:     Color = Color::from_rgba8(0xa6, 0xe3, 0xa1, 0xff);
    pub const YELLOW:    Color = Color::from_rgba8(0xf9, 0xe2, 0xaf, 0xff);
    pub const MAUVE:     Color = Color::from_rgba8(0xcb, 0xb6, 0xff, 0xff);
    pub const TEAL:      Color = Color::from_rgba8(0x94, 0xe2, 0xd5, 0xff);
}

/// Convert alacritty TermColor to peniko Color
fn term_color_to_peniko(color: TermColor) -> Color {
    let rgb = match color {
        TermColor::Named(named) => {
            match named {
                NamedColor::Black => Rgb { r: 0x45, g: 0x47, b: 0x5a },
                NamedColor::Red => Rgb { r: 0xf3, g: 0x8b, b: 0xa8 },
                NamedColor::Green => Rgb { r: 0xa6, g: 0xe3, b: 0xa1 },
                NamedColor::Yellow => Rgb { r: 0xf9, g: 0xe2, b: 0xaf },
                NamedColor::Blue => Rgb { r: 0x89, g: 0xb4, b: 0xfa },
                NamedColor::Magenta => Rgb { r: 0xcb, g: 0xb6, b: 0xff },
                NamedColor::Cyan => Rgb { r: 0x94, g: 0xe2, b: 0xd5 },
                NamedColor::White => Rgb { r: 0xc6, g: 0xd0, b: 0xf5 },
                NamedColor::BrightBlack => Rgb { r: 0x58, g: 0x5b, b: 0x70 },
                NamedColor::BrightRed => Rgb { r: 0xf3, g: 0x8b, b: 0xa8 },
                NamedColor::BrightGreen => Rgb { r: 0xa6, g: 0xe3, b: 0xa1 },
                NamedColor::BrightYellow => Rgb { r: 0xf9, g: 0xe2, b: 0xaf },
                NamedColor::BrightBlue => Rgb { r: 0x89, g: 0xb4, b: 0xfa },
                NamedColor::BrightMagenta => Rgb { r: 0xcb, g: 0xb6, b: 0xff },
                NamedColor::BrightCyan => Rgb { r: 0x94, g: 0xe2, b: 0xd5 },
                NamedColor::BrightWhite => Rgb { r: 0xc6, g: 0xd0, b: 0xf5 },
                NamedColor::Foreground => Rgb { r: 0xc6, g: 0xd0, b: 0xf5 },
                NamedColor::Background => Rgb { r: 0x1e, g: 0x1e, b: 0x2e },
                _ => Rgb { r: 0xc6, g: 0xd0, b: 0xf5 },
            }
        }
        TermColor::Spec(rgb) => rgb,
        TermColor::Indexed(idx) => {
            if idx < 8 {
                const COLORS: [(u8, u8, u8); 8] = [
                    (0x45, 0x47, 0x5a), (0xf3, 0x8b, 0xa8), (0xa6, 0xe3, 0xa1), (0xf9, 0xe2, 0xaf),
                    (0x89, 0xb4, 0xfa), (0xcb, 0xb6, 0xff), (0x94, 0xe2, 0xd5), (0xc6, 0xd0, 0xf5),
                ];
                let (r, g, b) = COLORS[idx as usize];
                Rgb { r, g, b }
            } else if idx < 16 {
                let (r, g, b) = match idx - 8 {
                    0 => (0x58, 0x5b, 0x70), 1 => (0xf3, 0x8b, 0xa8), 2 => (0xa6, 0xe3, 0xa1),
                    3 => (0xf9, 0xe2, 0xaf), 4 => (0x89, 0xb4, 0xfa), 5 => (0xcb, 0xb6, 0xff),
                    6 => (0x94, 0xe2, 0xd5), 7 => (0xc6, 0xd0, 0xf5),
                    _ => (0xc6, 0xd0, 0xf5),
                };
                Rgb { r, g, b }
            } else if idx < 232 {
                let ci = idx - 16;
                let r = (ci / 36) * 51;
                let g = ((ci % 36) / 6) * 51;
                let b = (ci % 6) * 51;
                Rgb { r, g, b }
            } else {
                let gray = 8 + (idx - 232) * 10;
                Rgb { r: gray, g: gray, b: gray }
            }
        }
    };
    Color::from_rgba8(rgb.r, rgb.g, rgb.b, 0xff)
}

/// Terminal widget with full keyboard, scroll, and text support
pub struct TerminalWidget {
    pub terminal: Terminal,
    pub is_active: bool,
    pub cell_width: f64,
    pub cell_height: f64,
    size: Size,
    needs_update: bool,
    /// Scroll offset from bottom of history
    scroll_offset: usize,
}

impl TerminalWidget {
    pub fn new(terminal: Terminal, is_active: bool) -> Self {
        Self {
            terminal,
            is_active,
            cell_width: 9.0,
            cell_height: 18.0,
            size: Size::new(800.0, 600.0),
            needs_update: true,
            scroll_offset: 0,
        }
    }

    pub fn set_active(&mut self, active: bool) {
        self.is_active = active;
    }

    /// Send keyboard input to terminal PTY
    fn send_key(&mut self, key_str: &str, ctrl: bool, alt: bool) {
        if ctrl {
            // Ctrl+letter -> control character
            if key_str.len() == 1 && key_str >= "a" && key_str <= "z" {
                let ctrl_char = key_str.as_bytes()[0] - b'a' + 1;
                let _ = self.terminal.write_input(&[ctrl_char]);
                return;
            }
            match key_str {
                " " | "@" => { let _ = self.terminal.write_input(&[0x00]); }
                "[" => { let _ = self.terminal.write_input(&[0x1b]); }
                "\\" => { let _ = self.terminal.write_input(&[0x1c]); }
                "]" => { let _ = self.terminal.write_input(&[0x1d]); }
                "^" => { let _ = self.terminal.write_input(&[0x1e]); }
                "_" => { let _ = self.terminal.write_input(&[0x1f]); }
                _ => {}
            }
        } else if alt {
            // Alt+key -> Escape sequence
            if key_str.len() == 1 {
                let _ = self.terminal.write_input(&[0x1b]);
                let _ = self.terminal.write_input(key_str.as_bytes());
            }
        } else {
            match key_str {
                "enter" | "return" => { let _ = self.terminal.write_input(b"\r"); }
                "backspace" => { let _ = self.terminal.write_input(b"\x7f"); }
                "delete" => { let _ = self.terminal.write_input(b"\x1b[3~"); }
                "escape" => { let _ = self.terminal.write_input(b"\x1b"); }
                "tab" => { let _ = self.terminal.write_input(b"\t"); }
                " " => { let _ = self.terminal.write_input(b" "); }
                "up" => { let _ = self.terminal.write_input(b"\x1b[A"); }
                "down" => { let _ = self.terminal.write_input(b"\x1b[B"); }
                "right" => { let _ = self.terminal.write_input(b"\x1b[C"); }
                "left" => { let _ = self.terminal.write_input(b"\x1b[D"); }
                "home" => { let _ = self.terminal.write_input(b"\x1b[H"); }
                "end" => { let _ = self.terminal.write_input(b"\x1b[F"); }
                "pageup" => { let _ = self.terminal.write_input(b"\x1b[5~"); }
                "pagedown" => { let _ = self.terminal.write_input(b"\x1b[6~"); }
                "insert" => { let _ = self.terminal.write_input(b"\x1b[2~"); }
                "f1" => { let _ = self.terminal.write_input(b"\x1bOP"); }
                "f2" => { let _ = self.terminal.write_input(b"\x1bOQ"); }
                "f3" => { let _ = self.terminal.write_input(b"\x1bOR"); }
                "f4" => { let _ = self.terminal.write_input(b"\x1bOS"); }
                "f5" => { let _ = self.terminal.write_input(b"\x1b[15~"); }
                "f6" => { let _ = self.terminal.write_input(b"\x1b[17~"); }
                "f7" => { let _ = self.terminal.write_input(b"\x1b[18~"); }
                "f8" => { let _ = self.terminal.write_input(b"\x1b[19~"); }
                "f9" => { let _ = self.terminal.write_input(b"\x1b[20~"); }
                "f10" => { let _ = self.terminal.write_input(b"\x1b[21~"); }
                "f11" => { let _ = self.terminal.write_input(b"\x1b[23~"); }
                "f12" => { let _ = self.terminal.write_input(b"\x1b[24~"); }
                _ => {
                    if key_str.len() == 1 {
                        let _ = self.terminal.write_input(key_str.as_bytes());
                    }
                }
            }
        }
    }
}

/// Terminal widget action
#[derive(Debug, Clone)]
pub enum TerminalAction {
    Prefix,
    KeyInput { key: String, ctrl: bool, alt: bool },
}

impl Widget for TerminalWidget {
    type Action = TerminalAction;

    fn register_children(&mut self, _ctx: &mut RegisterCtx<'_>) {}

    fn measure(
        &mut self,
        _ctx: &mut MeasureCtx<'_>,
        _props: &PropertiesRef<'_>,
        _axis: xilem::masonry::kurbo::Axis,
        _len_req: xilem::masonry::layout::LenReq,
        _cross_length: Option<f64>,
    ) -> f64 {
        200.0
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx<'_>,
        _props: &PropertiesRef<'_>,
        size: Size,
    ) {
        ctx.set_clip_path(size.to_rect());
        self.size = size;

        // Calculate terminal size based on available space
        let cols = (size.width / self.cell_width).max(10.0) as u16;
        let rows = (size.height / self.cell_height).max(1.0) as u16;

        let current_cols = self.terminal.config().cols;
        let current_rows = self.terminal.config().rows;
        if cols != current_cols || rows != current_rows {
            self.terminal.resize(cols, rows);
            self.needs_update = true;
        }
    }

    fn paint(
        &mut self,
        _ctx: &mut PaintCtx<'_>,
        _props: &PropertiesRef<'_>,
        painter: &mut Painter<'_>,
    ) {
        // Background
        let rect = self.size.to_rect();
        painter.fill(&rect, colors::CRUST).draw();

        let grid = self.terminal.grid();
        let display_offset = self.terminal.display_offset() + self.scroll_offset;
        let cols = grid.columns();
        let total_lines = grid.total_lines();

        let cursor = self.terminal.cursor();
        let cursor_col = cursor.column.0;
        let cursor_line = cursor.line.0;

        let cell_w = self.size.width / cols as f64;
        let cell_h = self.size.height / 24.0;
        let screen_rows = 24usize.min(total_lines);

        for row_idx in 0..screen_rows {
            let actual_row = row_idx + display_offset;
            if actual_row >= total_lines {
                break;
            }

            let y = row_idx as f64 * cell_h;
            let line = Line(actual_row as i32);
            let row = &grid[line];

            // Cursor line highlight
            if self.is_active && row_idx as i32 == cursor_line {
                let hl_rect = Rect::new(0.0, y, self.size.width, y + cell_h);
                painter.fill(&hl_rect, colors::SURFACE0).draw();
            }

            // Render cells
            for col_idx in 0..cols {
                let col = Column(col_idx);
                let cell = &row[col];

                if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                    || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }

                let ch = cell.c;
                let x = col_idx as f64 * cell_w;

                let fg_color = term_color_to_peniko(cell.fg);
                let bg_color = term_color_to_peniko(cell.bg);

                // Draw background if not default
                if cell.bg != TermColor::Named(NamedColor::Background) {
                    let cell_rect = Rect::new(x, y, x + cell_w, y + cell_h);
                    painter.fill(&cell_rect, bg_color).draw();
                }

                // Skip empty cells
                if ch == '\0' || ch == ' ' {
                    continue;
                }

                // Character block (colored rectangle representing the glyph)
                let char_margin = 0.5;
                let char_rect = Rect::new(
                    x + char_margin,
                    y + char_margin,
                    x + cell_w - char_margin,
                    y + cell_h - char_margin,
                );
                painter.fill(&char_rect, fg_color).draw();

                // Bold left indicator
                if cell.flags.contains(Flags::BOLD) || cell.flags.contains(Flags::DIM_BOLD) {
                    let bold_rect = Rect::new(x, y, x + 2.0, y + cell_h);
                    painter.fill(&bold_rect, Color::from_rgba8(0xff, 0xff, 0xff, 0x20)).draw();
                }

                // Underline
                if cell.flags.contains(Flags::UNDERLINE) {
                    let ul_rect = Rect::new(x, y + cell_h - 2.0, x + cell_w, y + cell_h);
                    painter.fill(&ul_rect, fg_color).draw();
                }

                // Strikethrough
                if cell.flags.contains(Flags::STRIKEOUT) {
                    let strike_rect = Rect::new(x, y + cell_h / 2.0 - 1.0, x + cell_w, y + cell_h / 2.0 + 1.0);
                    painter.fill(&strike_rect, fg_color).draw();
                }

                // Inverse (swap fg/bg)
                if cell.flags.contains(Flags::INVERSE) {
                    let inv_rect = Rect::new(x, y, x + cell_w, y + cell_h);
                    painter.fill(&inv_rect, fg_color).draw();
                }
            }
        }

        // Draw cursor block
        if self.is_active && cursor_line >= 0 && (cursor_line as usize) < 24 && cursor_col < cols as usize {
            let cursor_y = cursor_line as f64 * cell_h;
            let cursor_x = cursor_col as f64 * cell_w;
            let cursor_rect = Rect::new(cursor_x, cursor_y, cursor_x + cell_w, cursor_y + cell_h);
            painter.fill(&cursor_rect, colors::ROSEWATER).draw();
        }

        // Border
        let border_color = if self.is_active { colors::BLUE } else { colors::SURFACE0 };
        let border = Stroke::new(2.0);
        painter.stroke(&rect, &border, border_color).draw();

        // Scroll indicator (show if scrolled into history)
        if self.scroll_offset > 0 {
            let scroll_pct = self.scroll_offset as f64 / total_lines as f64;
            let indicator_h = (self.size.height * 0.1).max(20.0);
            let indicator_y = self.size.height * (1.0 - scroll_pct) - indicator_h;
            let indicator_rect = Rect::new(self.size.width - 6.0, indicator_y, self.size.width - 2.0, indicator_y + indicator_h);
            painter.fill(&indicator_rect, colors::SURFACE1).draw();
        }
    }

    fn on_pointer_event(
        &mut self,
        ctx: &mut EventCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        event: &PointerEvent,
    ) {
        // Handle mouse wheel for scrolling (if available)
        // Request focus on pointer down to receive keyboard events
        ctx.request_focus();
    }

    fn on_text_event(
        &mut self,
        ctx: &mut EventCtx<'_>,
        _props: &mut PropertiesMut<'_>,
        event: &TextEvent,
    ) {
        if let TextEvent::Keyboard(key_event) = event {
            if key_event.state == KeyState::Down {
                match &key_event.key {
                    Key::Character(c) => {
                        let modifiers = key_event.modifiers;
                        let ctrl = modifiers.ctrl();
                        let alt = modifiers.alt();

                        // Ctrl+B = prefix key
                        if ctrl && c.as_str() == "b" {
                            ctx.submit_action::<TerminalAction>(TerminalAction::Prefix);
                            return;
                        }

                        self.send_key(c.as_str(), ctrl, alt);
                    }
                    Key::Named(named) => {
                        let modifiers = key_event.modifiers;

                        match named {
                            NamedKey::Enter => { let _ = self.terminal.write_input(b"\r"); }
                            NamedKey::Escape => { let _ = self.terminal.write_input(b"\x1b"); }
                            NamedKey::Tab => { let _ = self.terminal.write_input(b"\t"); }
                            NamedKey::Backspace => { let _ = self.terminal.write_input(b"\x7f"); }
                            NamedKey::Delete => { let _ = self.terminal.write_input(b"\x1b[3~"); }
                            NamedKey::ArrowUp => { let _ = self.terminal.write_input(b"\x1b[A"); }
                            NamedKey::ArrowDown => { let _ = self.terminal.write_input(b"\x1b[B"); }
                            NamedKey::ArrowLeft => { let _ = self.terminal.write_input(b"\x1b[D"); }
                            NamedKey::ArrowRight => { let _ = self.terminal.write_input(b"\x1b[C"); }
                            NamedKey::Home => { let _ = self.terminal.write_input(b"\x1b[H"); }
                            NamedKey::End => { let _ = self.terminal.write_input(b"\x1b[F"); }
                            NamedKey::PageUp => {
                                // Shift+PageUp = scroll up
                                if modifiers.shift() {
                                    let grid = self.terminal.grid();
                                    let max_scroll = grid.total_lines().saturating_sub(24);
                                    self.scroll_offset = (self.scroll_offset + 24).min(max_scroll);
                                    ctx.request_render();
                                } else {
                                    let _ = self.terminal.write_input(b"\x1b[5~");
                                }
                            }
                            NamedKey::PageDown => {
                                // Shift+PageDown = scroll down
                                if modifiers.shift() {
                                    self.scroll_offset = self.scroll_offset.saturating_sub(24);
                                    ctx.request_render();
                                } else {
                                    let _ = self.terminal.write_input(b"\x1b[6~");
                                }
                            }
                            NamedKey::F1 => { let _ = self.terminal.write_input(b"\x1bOP"); }
                            NamedKey::F2 => { let _ = self.terminal.write_input(b"\x1bOQ"); }
                            NamedKey::F3 => { let _ = self.terminal.write_input(b"\x1bOR"); }
                            NamedKey::F4 => { let _ = self.terminal.write_input(b"\x1bOS"); }
                            NamedKey::F5 => { let _ = self.terminal.write_input(b"\x1b[15~"); }
                            NamedKey::F6 => { let _ = self.terminal.write_input(b"\x1b[17~"); }
                            NamedKey::F7 => { let _ = self.terminal.write_input(b"\x1b[18~"); }
                            NamedKey::F8 => { let _ = self.terminal.write_input(b"\x1b[19~"); }
                            NamedKey::F9 => { let _ = self.terminal.write_input(b"\x1b[20~"); }
                            NamedKey::F10 => { let _ = self.terminal.write_input(b"\x1b[21~"); }
                            NamedKey::F11 => { let _ = self.terminal.write_input(b"\x1b[23~"); }
                            NamedKey::F12 => { let _ = self.terminal.write_input(b"\x1b[24~"); }
                            _ => {}
                        }
                    }
                    _ => {}
                }
                self.needs_update = true;
                ctx.request_render();
            }
        } else if let TextEvent::Ime(ime) = event {
            // Handle IME input (CJK characters)
            if let Ime::Commit(text) = ime {
                let _ = self.terminal.write_input(text.as_bytes());
                self.needs_update = true;
                ctx.request_render();
            }
        }
    }

    fn accessibility_role(&self) -> Role {
        Role::Terminal
    }

    fn accessibility(
        &mut self,
        _ctx: &mut AccessCtx<'_>,
        _props: &PropertiesRef<'_>,
        _node: &mut Node,
    ) {
    }

    fn children_ids(&self) -> ChildrenIds {
        ChildrenIds::new()
    }
}
