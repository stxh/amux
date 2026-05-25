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

    // Sleek dimmed border colors
    pub const ACTIVE_BORDER:   Color = Color::from_rgba8(0x3b, 0x59, 0x7f, 0xff); // Classy muted steel blue
    pub const INACTIVE_BORDER: Color = Color::from_rgba8(0x22, 0x22, 0x30, 0xff); // Elegant subtle dark grey
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
    const MARGIN: f64 = 12.0;

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

    // Measure actual monospace character width from font metrics
    const FONT_SIZE: f32 = 18.0;

    // Build probe layouts to measure character advance dimensions.
    // Use a Latin-only probe for cell_w and a mixed Latin+CJK probe for
    // cell_h, because CJK fallback fonts (e.g. PingFang on macOS) often have
    // taller metrics than the primary monospace font, causing Chinese glyphs
    // to overflow the selection-highlight rect.
    let cell_w = {
        let probe = "MMMMMMMMMM";
        let mut mb = lcx.ranged_builder(fcx, probe, 1.0, true);
        mb.push_default(StyleProperty::FontFamily(FontFamily::Single(
            FontFamilyName::Generic(GenericFamily::Monospace),
        )));
        mb.push_default(StyleProperty::FontSize(FONT_SIZE));
        let mut ml = mb.build(probe);
        ml.break_all_lines(None);
        ml.width() as f64 / 10.0
    };
    let (cell_h, baseline_ascent) = {
        let probe = "MMMMMMMMMM你好"; // Mixed to get the taller metrics
        let mut mb = lcx.ranged_builder(fcx, probe, 1.0, true);
        mb.push_default(StyleProperty::FontFamily(FontFamily::Single(
            FontFamilyName::Generic(GenericFamily::Monospace),
        )));
        mb.push_default(StyleProperty::FontSize(FONT_SIZE));
        let mut ml = mb.build(probe);
        ml.break_all_lines(None);
        let h = ml.height() as f64;
        // Measure the ascent (distance from line top to baseline) from the
        // mixed-script probe, which reflects the taller CJK metrics.
        let ascent = ml.lines().next().map_or(h * 0.8, |l| l.metrics().baseline as f64);
        (h, ascent)
    };

    // Calculate how many rows we can actually display
    let max_displayable_rows = ((size.height - 2.0 * MARGIN).max(0.0) / cell_h).floor() as usize;
    let display_lines = screen_lines.min(max_displayable_rows);

    // Render visible rows
    for row_idx in 0..display_lines {
        let y = offset_y + MARGIN + row_idx as f64 * cell_h;
        let line_num = row_idx as i32 - display_offset as i32;
        let line = Line(line_num);

        // Compute cursor row match once per row
        let cursor_screen_row = cursor_line_abs + display_offset as i32;
        let is_cursor_row = cursor_screen_row == row_idx as i32;

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
            let mut fg_color = if is_default_fg(cell.fg) {
                colors::TEXT
            } else {
                term_color_to_peniko(cell.fg)
            };

            // If this is the cursor cell, invert colors (cursor bg is TEXT, text fg is CRUST)
            if is_active && is_cursor_row && col_idx == cursor_col {
                fg_color = colors::CRUST;
            } else if terminal.is_cell_selected(line_num, col_idx) {
                fg_color = colors::CRUST;
            }

            char_colors.push(fg_color);
        }

        // Draw backgrounds: merge contiguous selected cells into one rect
        // to eliminate 1px anti-aliased seams between adjacent cells.
        let bg_row_y = offset_y + MARGIN + row_idx as f64 * cell_h;
        {
            // Non-default backgrounds (per-cell, can't easily merge)
            for col_idx in 0..cols {
                let col = Column(col_idx);
                let cell = &row[col];

                if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                    || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }

                if !terminal.is_cell_selected(line_num, col_idx)
                    && cell.bg != TermColor::Named(NamedColor::Background)
                {
                    let bx = offset_x + MARGIN + col_idx as f64 * cell_w;
                    let bg_color = term_color_to_peniko(cell.bg);
                    let cell_rect = Rect::new(bx, bg_row_y, bx + cell_w, bg_row_y + cell_h);
                    painter.fill(&cell_rect, bg_color).draw();
                    if cell.flags.contains(Flags::WIDE_CHAR) && col_idx + 1 < cols {
                        let sx = offset_x + MARGIN + (col_idx + 1) as f64 * cell_w;
                        let spacer_rect = Rect::new(sx, bg_row_y, sx + cell_w, bg_row_y + cell_h);
                        painter.fill(&spacer_rect, bg_color).draw();
                    }
                }
            }

            // Merge contiguous selected cells into single rectangles
            let mut sel_start: Option<usize> = None;
            for col_idx in 0..=cols {
                let is_sel = col_idx < cols && terminal.is_cell_selected(line_num, col_idx);
                match (sel_start, is_sel) {
                    (Some(start), true) => {} // continue
                    (Some(start), false) => {
                        // End of selection run — draw one merged rect
                        let bx = offset_x + MARGIN + start as f64 * cell_w;
                        let ex = offset_x + MARGIN + col_idx as f64 * cell_w;
                        let sel_rect = Rect::new(bx, bg_row_y, ex, bg_row_y + cell_h);
                        painter.fill(&sel_rect, colors::BLUE).draw();
                        sel_start = None;
                    }
                    (None, true) => {
                        sel_start = Some(col_idx);
                    }
                    (None, false) => {}
                }
            }
        }

        // Underline / strikethrough
        for col_idx in 0..cols {
            let col = Column(col_idx);
            let cell = &row[col];
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            let ux = offset_x + MARGIN + col_idx as f64 * cell_w;
            if cell.flags.contains(Flags::UNDERLINE) {
                let fg_color = term_color_to_peniko(cell.fg);
                let ul_rect = Rect::new(ux, bg_row_y + cell_h - 2.0, ux + cell_w, bg_row_y + cell_h);
                painter.fill(&ul_rect, fg_color).draw();
            }
            if cell.flags.contains(Flags::STRIKEOUT) {
                let fg_color = term_color_to_peniko(cell.fg);
                let strike_rect = Rect::new(ux, bg_row_y + cell_h / 2.0 - 1.0, ux + cell_w, bg_row_y + cell_h / 2.0 + 1.0);
                painter.fill(&strike_rect, fg_color).draw();
            }
        }

        // Draw cursor block for this row before text
        if is_active && is_cursor_row && cursor_col < cols as usize {
            let cursor_cell = &row[Column(cursor_col)];
            let cursor_w = if cursor_cell.flags.contains(Flags::WIDE_CHAR) {
                cell_w * 2.0
            } else {
                cell_w
            };
            let cursor_x = offset_x + MARGIN + cursor_col as f64 * cell_w;
            let cursor_rect = Rect::new(cursor_x, bg_row_y, cursor_x + cursor_w, bg_row_y + cell_h);
            painter.fill(&cursor_rect, colors::TEXT).draw();
        }

        // Render each non-spacer character at its fixed cell position.
        // All characters share the same baseline (baseline_ascent from the
        // mixed-script probe) so Latin and CJK glyphs align correctly.
        for col_idx in 0..cols {
            let col = Column(col_idx);
            let cell = &row[col];

            if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }

            let ch = match cell.c {
                '\0' => ' ',
                c if c.is_control() && c != '\t' => ' ',
                c => c,
            };

            if ch == ' ' {
                continue;
            }

            let fg_color = char_colors[col_idx];
            let x = offset_x + MARGIN + col_idx as f64 * cell_w;

            let s = ch.to_string();
            let mut builder = lcx.ranged_builder(fcx, &s, 1.0, true);
            builder.push_default(StyleProperty::FontFamily(FontFamily::Single(
                FontFamilyName::Generic(GenericFamily::Monospace),
            )));
            builder.push_default(StyleProperty::FontSize(FONT_SIZE));

            if fg_color != colors::TEXT {
                let brush = Brush::Solid(fg_color);
                builder.push(
                    StyleProperty::Brush(xilem::masonry::core::BrushIndex(1)),
                    0..s.len(),
                );
                let mut layout = builder.build(&s);
                layout.break_all_lines(None);
                let char_ascent = layout.lines().next().map_or(baseline_ascent, |l| l.metrics().baseline as f64);
                let ty = y + baseline_ascent - char_ascent;
                let transform = Affine::translate((x, ty));
                render_text(painter, transform, &layout, &[Brush::Solid(colors::TEXT), brush], true);
            } else {
                let mut layout = builder.build(&s);
                layout.break_all_lines(None);
                let char_ascent = layout.lines().next().map_or(baseline_ascent, |l| l.metrics().baseline as f64);
                let ty = y + baseline_ascent - char_ascent;
                let transform = Affine::translate((x, ty));
                render_text(painter, transform, &layout, &[Brush::Solid(colors::TEXT)], true);
            }
        }
    }
    // Border
    let border_color = if is_active { colors::ACTIVE_BORDER } else { colors::INACTIVE_BORDER };
    let border = Stroke::new(2.0);
    painter.stroke(&rect, &border, border_color).draw();

    // Scrollbar — always visible track with proportional thumb
    {
        let track_width = 8.0;
        let track_x = offset_x + size.width - track_width - MARGIN;
        let track_rect = Rect::new(track_x, offset_y + MARGIN, track_x + track_width, offset_y + size.height - MARGIN);
        // Track background (semi-transparent)
        painter.fill(&track_rect, Color::from_rgba8(0x18, 0x18, 0x25, 0x80)).draw();

        // Thumb — size proportional to viewport/total, position proportional to scroll offset
        if total_lines > 0 {
            let scrollable_h = size.height - 2.0 * MARGIN;
            let viewport_ratio = (screen_lines as f64 / total_lines as f64).min(1.0);
            let thumb_h = (scrollable_h * viewport_ratio).max(20.0);

            // Current view position within total scrollable range
            let max_scroll = (total_lines as isize - screen_lines as isize).max(0) as f64;
            let scroll_progress = if max_scroll > 0.0 {
                1.0 - (pane_state.scroll_offset as f64 / max_scroll)
            } else {
                1.0
            };
            let thumb_y = offset_y + MARGIN + (scrollable_h - thumb_h) * scroll_progress.clamp(0.0, 1.0);
            let thumb_rect = Rect::new(track_x + 1.0, thumb_y, track_x + track_width - 1.0, thumb_y + thumb_h);
            painter.fill(&thumb_rect, colors::SURFACE1).draw();
        }
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
