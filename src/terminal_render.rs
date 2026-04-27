/// Terminal canvas rendering - renders terminal grid onto a canvas surface
///
/// This module provides functions to render terminal grids using Xilem's canvas pattern.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::{Color as TermColor, NamedColor, Rgb};

use xilem::masonry::core::render_text;
use xilem::masonry::imaging::{Painter, record::Scene};
use xilem::masonry::kurbo::{Affine, Rect, Size, Stroke};
use xilem::masonry::parley::style::{FontFamily, FontFamilyName, GenericFamily, StyleProperty};
use xilem::masonry::parley::{Alignment, AlignmentOptions, Layout};
use xilem::masonry::peniko::{Brush, Color};

use crate::terminal::Terminal;
use crate::app::App;

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

/// Terminal pane state (maintained separately from the terminal itself)
#[derive(Clone)]
pub struct TerminalPaneState {
    /// Scroll offset from bottom of history
    pub scroll_offset: usize,
    /// Whether this pane has keyboard focus
    pub is_focused: bool,
}

impl TerminalPaneState {
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
            is_focused: false,
        }
    }
}

/// Render a terminal onto a canvas with an offset
///
/// This function renders a terminal at a specific position within the canvas,
/// allowing multiple panes to be rendered in their layout positions.
pub fn render_terminal_with_offset(
    painter: &mut Painter<'_, Scene>,
    terminal: &Terminal,
    pane_state: &TerminalPaneState,
    is_active: bool,
    size: Size,
    offset_x: f64,
    offset_y: f64,
    fcx: &mut xilem::masonry::parley::FontContext,
    lcx: &mut xilem::masonry::parley::LayoutContext<xilem::masonry::core::BrushIndex>,
) {
    // Background for this pane
    let rect = Rect::new(offset_x, offset_y, offset_x + size.width, offset_y + size.height);
    painter.fill(&rect, colors::CRUST).draw();

    let grid = terminal.grid();
    let display_offset = terminal.display_offset() + pane_state.scroll_offset;
    let cols = grid.columns();
    let total_lines = grid.total_lines();
    let screen_lines = grid.screen_lines();

    let cursor = terminal.cursor();
    let cursor_col = cursor.column.0;
    let cursor_line = cursor.line.0;

    // Calculate cell dimensions based on actual screen lines
    let cell_w = size.width / cols as f64;
    let cell_h = size.height / screen_lines as f64;

    for row_idx in 0..screen_lines {
        let actual_row = row_idx + display_offset;
        if actual_row >= total_lines {
            break;
        }

        let y = offset_y + row_idx as f64 * cell_h;
        let line = Line(actual_row as i32);
        let row = &grid[line];

        // Cursor line highlight
        if is_active && row_idx as i32 == cursor_line {
            let hl_rect = Rect::new(offset_x, y, offset_x + size.width, y + cell_h);
            painter.fill(&hl_rect, colors::SURFACE0).draw();
        }

        // Build text string for this row
        let mut row_text = String::with_capacity(cols);

        for col_idx in 0..cols {
            let col = Column(col_idx);
            let cell = &row[col];

            if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
            {
                // Add space placeholder for wide char
                row_text.push(' ');
                continue;
            }

            let ch = if cell.c == '\0' { ' ' } else { cell.c };
            row_text.push(ch);
        }

        // Draw background cells that have non-default colors
        for col_idx in 0..cols {
            let col = Column(col_idx);
            let cell = &row[col];

            if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }

            if cell.bg != TermColor::Named(NamedColor::Background) {
                let x = offset_x + col_idx as f64 * cell_w;
                let bg_color = term_color_to_peniko(cell.bg);
                let cell_rect = Rect::new(x, y, x + cell_w, y + cell_h);
                painter.fill(&cell_rect, bg_color).draw();
            }

            // Bold indicator
            if cell.flags.contains(Flags::BOLD) || cell.flags.contains(Flags::DIM_BOLD) {
                let x = offset_x + col_idx as f64 * cell_w;
                let bold_rect = Rect::new(x, y, x + 2.0, y + cell_h);
                painter.fill(&bold_rect, Color::from_rgba8(0xff, 0xff, 0xff, 0x20)).draw();
            }

            // Underline
            if cell.flags.contains(Flags::UNDERLINE) {
                let x = offset_x + col_idx as f64 * cell_w;
                let fg_color = term_color_to_peniko(cell.fg);
                let ul_rect = Rect::new(x, y + cell_h - 2.0, x + cell_w, y + cell_h);
                painter.fill(&ul_rect, fg_color).draw();
            }

            // Strikethrough
            if cell.flags.contains(Flags::STRIKEOUT) {
                let x = offset_x + col_idx as f64 * cell_w;
                let fg_color = term_color_to_peniko(cell.fg);
                let strike_rect = Rect::new(x, y + cell_h / 2.0 - 1.0, x + cell_w, y + cell_h / 2.0 + 1.0);
                painter.fill(&strike_rect, fg_color).draw();
            }
        }

        // Render text for this row using parley
        if !row_text.is_empty() {
            // Use cell_h directly as font size - should give correct sizing
            let font_size = cell_h as f32 * 0.7;
            let mut builder = lcx.ranged_builder(fcx, &row_text, 1.0, true);
            builder.push_default(StyleProperty::FontFamily(FontFamily::Single(
                FontFamilyName::Generic(GenericFamily::Monospace),
            )));
            builder.push_default(StyleProperty::FontSize(font_size));
            
            let mut layout = builder.build(&row_text);
            layout.break_all_lines(None);
            layout.align(None, Alignment::Start, AlignmentOptions::default());
            
            // Position baseline at about 78% of cell from top
            let text_y_base = y + cell_h * 0.78;
            let transform = Affine::translate((offset_x, text_y_base));
            
            // Use default text color
            let brushes = &[Brush::Solid(colors::TEXT)];
            render_text(painter, transform, &layout, brushes, true);
        }
    }

    // Draw cursor block
    if is_active && cursor_line >= 0 && (cursor_line as usize) < screen_lines && cursor_col < cols as usize {
        let cursor_y = offset_y + cursor_line as f64 * cell_h;
        let cursor_x = offset_x + cursor_col as f64 * cell_w;
        let cursor_rect = Rect::new(cursor_x, cursor_y, cursor_x + cell_w, cursor_y + cell_h);
        painter.fill(&cursor_rect, colors::ROSEWATER).draw();
    }

    // Border
    let border_color = if is_active { colors::BLUE } else { colors::SURFACE0 };
    let border = Stroke::new(2.0);
    painter.stroke(&rect, &border, border_color).draw();

    // Scroll indicator (show if scrolled into history)
    if pane_state.scroll_offset > 0 {
        let scroll_pct = pane_state.scroll_offset as f64 / total_lines as f64;
        let indicator_h = (size.height * 0.1).max(20.0);
        let indicator_y = offset_y + size.height * (1.0 - scroll_pct) - indicator_h;
        let indicator_rect = Rect::new(offset_x + size.width - 6.0, indicator_y, offset_x + size.width - 2.0, indicator_y + indicator_h);
        painter.fill(&indicator_rect, colors::SURFACE1).draw();
    }
}

/// Render a terminal onto a canvas (without offset - fills the entire canvas)
pub fn render_terminal(
    painter: &mut Painter<'_, Scene>,
    terminal: &Terminal,
    pane_state: &TerminalPaneState,
    is_active: bool,
    size: Size,
    fcx: &mut xilem::masonry::parley::FontContext,
    lcx: &mut xilem::masonry::parley::LayoutContext<xilem::masonry::core::BrushIndex>,
) {
    render_terminal_with_offset(painter, terminal, pane_state, is_active, size, 0.0, 0.0, fcx, lcx)
}

/// Get the focused pane index
pub fn get_focused_pane(app: &App) -> Option<usize> {
    app.panes.iter().position(|p| p.is_active)
}
