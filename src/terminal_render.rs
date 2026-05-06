/// Terminal canvas rendering - renders terminal grid onto a canvas surface
///
/// This module provides functions to render terminal grids using Xilem's canvas pattern.
/// Supports per-character foreground/background coloring using parley's ranged_builder.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::{Color as TermColor, NamedColor, Rgb};

use xilem::masonry::core::render_text;
use xilem::masonry::imaging::{Painter, record::Scene};
use xilem::masonry::kurbo::{Affine, Rect, Size, Stroke};
use xilem::masonry::parley::style::{FontFamily, FontFamilyName, GenericFamily, StyleProperty};
use xilem::masonry::parley::{Alignment, AlignmentOptions};
use xilem::masonry::peniko::{Brush, Color};

use crate::terminal::Terminal;

/// Catppuccin Mocha color scheme
mod colors {
    use super::Color;
    pub const CRUST:     Color = Color::from_rgba8(0x11, 0x11, 0x1b, 0xff);
    pub const SURFACE0:  Color = Color::from_rgba8(0x31, 0x32, 0x44, 0xff);
    pub const SURFACE1:  Color = Color::from_rgba8(0x45, 0x47, 0x5a, 0xff);
    pub const TEXT:      Color = Color::from_rgba8(0xc6, 0xd0, 0xf5, 0xff);
    pub const BLUE:      Color = Color::from_rgba8(0x89, 0xb4, 0xfa, 0xff);
    pub const ROSEWATER: Color = Color::from_rgba8(0xf5, 0xe0, 0xdc, 0xff);
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

/// Check if a color is the default foreground
fn is_default_fg(color: TermColor) -> bool {
    matches!(color, TermColor::Named(NamedColor::Foreground))
}

/// Terminal pane state (maintained separately from the terminal itself)
#[derive(Clone)]
pub struct TerminalPaneState {
    /// Scroll offset from bottom of history
    pub scroll_offset: usize,
}

impl TerminalPaneState {
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
        }
    }
}

/// A color segment within a row — contiguous characters sharing the same fg color
struct ColorSegment {
    start_byte: usize,
    end_byte: usize,
    color: Color,
}

/// Render a terminal onto a canvas with an offset
///
/// This function renders a terminal at a specific position within the canvas,
/// allowing multiple panes to be rendered in their layout positions.
/// Now supports per-character foreground coloring via parley's ranged_builder.
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
    let cursor_line_abs = cursor.line.0;

    // Fixed cell dimensions
    const FONT_SIZE: f32 = 18.0;
    const CELL_WIDTH: f64 = 10.8;
    const CELL_HEIGHT: f64 = 25.2;

    let cell_w = CELL_WIDTH;
    let cell_h = CELL_HEIGHT;

    // Calculate how many rows we can actually display
    let max_displayable_rows = (size.height / cell_h).floor() as usize;
    let display_lines = screen_lines.min(max_displayable_rows);

    // Render visible rows
    for row_idx in 0..display_lines {
        let y = offset_y + row_idx as f64 * cell_h;
        let line_num = row_idx as i32 - display_offset as i32;
        let line = Line(line_num);

        // Skip lines that are out of bounds
        if line_num < grid.topmost_line().0 {
            continue;
        }

        let row = &grid[line];

        // Build text string and collect per-character color info
        let mut row_text = String::with_capacity(cols);
        let mut char_colors: Vec<Color> = Vec::with_capacity(cols);

        for col_idx in 0..cols {
            let col = Column(col_idx);
            let cell = &row[col];

            if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
            {
                row_text.push(' ');
                char_colors.push(colors::TEXT);
                continue;
            }

            let ch = match cell.c {
                '\0' => ' ',
                c if c.is_control() && c != '\t' => ' ',
                c => c,
            };
            row_text.push(ch);

            // Determine foreground color for this character
            let fg_color = if is_default_fg(cell.fg) {
                colors::TEXT
            } else {
                term_color_to_peniko(cell.fg)
            };
            char_colors.push(fg_color);
        }

        // Draw background cells with non-default colors
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

        // Render text with per-character coloring using ranged_builder
        if !row_text.is_empty() {
            let mut builder = lcx.ranged_builder(fcx, &row_text, 1.0, true);
            builder.push_default(StyleProperty::FontFamily(FontFamily::Single(
                FontFamilyName::Generic(GenericFamily::Monospace),
            )));
            builder.push_default(StyleProperty::FontSize(FONT_SIZE));

            // Build color segments: merge consecutive chars with same color
            let segments = build_color_segments(&row_text, &char_colors);

            // Collect brushes and push ranges
            let mut brushes: Vec<Brush> = Vec::new();
            // BrushIndex 0 = default text color
            brushes.push(Brush::Solid(colors::TEXT));

            for seg in &segments {
                if seg.color == colors::TEXT {
                    // Use default brush (index 0)
                    continue;
                }
                // Add this brush and push a range for it
                let brush_idx = brushes.len();
                brushes.push(Brush::Solid(seg.color));
                builder.push(
                    StyleProperty::Brush(xilem::masonry::core::BrushIndex(brush_idx)),
                    seg.start_byte..seg.end_byte,
                );
            }

            let mut layout = builder.build(&row_text);
            layout.break_all_lines(None);
            layout.align(None, Alignment::Start, AlignmentOptions::default());

            // Position baseline
            let text_y_base = y + cell_h * 0.78;
            let transform = Affine::translate((offset_x, text_y_base));

            render_text(painter, transform, &layout, &brushes, true);
        }
    }

    // Draw cursor block
    let cursor_screen_row = cursor_line_abs + display_offset as i32;
    if is_active && cursor_screen_row >= 0 && (cursor_screen_row as usize) < display_lines && cursor_col < cols as usize {
        let cursor_y = offset_y + cursor_screen_row as f64 * cell_h;
        let cursor_x = offset_x + cursor_col as f64 * cell_w;
        let cursor_rect = Rect::new(cursor_x, cursor_y, cursor_x + cell_w, cursor_y + cell_h);
        painter.fill(&cursor_rect, colors::ROSEWATER).draw();
    }

    // Border
    let border_color = if is_active { colors::BLUE } else { colors::SURFACE0 };
    let border = Stroke::new(2.0);
    painter.stroke(&rect, &border, border_color).draw();

    // Scroll indicator
    if pane_state.scroll_offset > 0 {
        let scroll_pct = pane_state.scroll_offset as f64 / total_lines as f64;
        let indicator_h = (size.height * 0.1).max(20.0);
        let indicator_y = offset_y + size.height * (1.0 - scroll_pct) - indicator_h;
        let indicator_rect = Rect::new(offset_x + size.width - 6.0, indicator_y, offset_x + size.width - 2.0, indicator_y + indicator_h);
        painter.fill(&indicator_rect, colors::SURFACE1).draw();
    }
}

/// Build color segments by merging consecutive characters with the same foreground color.
/// Returns byte-range segments for use with parley's ranged_builder.
fn build_color_segments(text: &str, char_colors: &[Color]) -> Vec<ColorSegment> {
    let mut segments = Vec::new();
    if char_colors.is_empty() {
        return segments;
    }

    let mut current_color = char_colors[0];
    let mut start_byte = 0usize;
    let mut char_idx = 0;
    let mut byte_pos = 0usize;

    for ch in text.chars() {
        let ch_len = ch.len_utf8();

        if char_idx < char_colors.len() && char_colors[char_idx] != current_color {
            // End previous segment
            segments.push(ColorSegment {
                start_byte,
                end_byte: byte_pos,
                color: current_color,
            });
            current_color = char_colors[char_idx];
            start_byte = byte_pos;
        }

        byte_pos += ch_len;
        char_idx += 1;
    }

    // Final segment
    if byte_pos > start_byte {
        segments.push(ColorSegment {
            start_byte,
            end_byte: byte_pos,
            color: current_color,
        });
    }

    segments
}
