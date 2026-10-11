/// Terminal canvas rendering - renders terminal grid onto a canvas surface
///
/// This module renders libghostty-vt render-state snapshots using Xilem's
/// canvas pattern. Supports per-character foreground/background coloring
/// using parley's ranged_builder.

use libghostty_vt::screen::CellWide;
use libghostty_vt::style::{RgbColor, Underline};
use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};

use xilem::masonry::core::render_text;
use xilem::masonry::imaging::{Painter, record::Scene};
use xilem::masonry::kurbo::{Affine, Rect, Size};
use xilem::masonry::parley::style::{FontFamily, StyleProperty};
use xilem::masonry::parley::FontContext;

use xilem::masonry::peniko::{Brush, Color};

use crate::terminal::Terminal;

/// Font fallback chain for terminal glyphs. Cascadia Mono (ships with Windows
/// Terminal) covers box-drawing, block elements, braille and check marks that
/// the generic monospace fallback (Consolas) lacks; the bundled Symbols Nerd
/// Font Mono supplies powerline/Nerd icons (U+E0B0.., U+F0..); Segoe UI
/// Symbol catches remaining dingbats; CJK resolves via system fallback.
pub fn term_font_family() -> FontFamily<'static> {
    FontFamily::Source(Cow::Borrowed(
        "Cascadia Mono, Consolas, Symbols Nerd Font Mono, Segoe UI Symbol, monospace",
    ))
}

static SYMBOL_FONT_REGISTERED: AtomicBool = AtomicBool::new(false);

/// Register the bundled Symbols Nerd Font Mono into parley's font collection
/// once. The collection lives in the app-wide FontContext, so registration
/// persists across frames.
pub fn ensure_symbol_font(fcx: &mut FontContext) {
    if SYMBOL_FONT_REGISTERED.load(Ordering::Relaxed) {
        return;
    }
    const DATA: &[u8] = include_bytes!("../assets/fonts/SymbolsNerdFontMono-Regular.ttf");
    let _ = fcx.collection.register_fonts(DATA.to_vec().into(), None);
    SYMBOL_FONT_REGISTERED.store(true, Ordering::Relaxed);
}

/// Catppuccin Mocha color scheme
mod colors {
    use super::Color;
    pub const CRUST:     Color = Color::from_rgba8(0x11, 0x11, 0x1b, 0xff);
    pub const SURFACE1:  Color = Color::from_rgba8(0x45, 0x47, 0x5a, 0xff);
    pub const TEXT:      Color = Color::from_rgba8(0xc6, 0xd0, 0xf5, 0xff);
    pub const BLUE:      Color = Color::from_rgba8(0x89, 0xb4, 0xfa, 0xff);
}

/// Bottom inset below the last text row. Top/left/right are flush. Shared by
/// the renderer, the grid-resize math and click/selection coordinate mapping.
pub const PANE_MARGIN: f64 = 6.0;

fn rgb_to_color(c: RgbColor) -> Color {
    Color::from_rgba8(c.r, c.g, c.b, 0xff)
}

/// Characters that must tile seamlessly across rows (box drawing, block
/// elements, braille). These get stretched to the full cell height when CJK
/// metrics inflate `cell_h` beyond the primary font's line height.
fn is_cell_filler(s: &str) -> bool {
    !s.is_empty()
        && s.chars().all(|ch| {
            matches!(ch,
                '\u{2500}'..='\u{259F}' | // box drawing + block elements
                '\u{2800}'..='\u{28FF}')  // braille patterns
        })
}

/// Position a glyph layout at its cell, sharing the row baseline. For
/// cell-filling glyphs with `stretch`, scale vertically about the baseline so
/// consecutive rows meet exactly.
fn glyph_transform(
    s: &str,
    x: f64,
    y: f64,
    baseline_ascent: f64,
    layout: &xilem::masonry::parley::Layout<xilem::masonry::core::BrushIndex>,
    stretch: bool,
    ratio: f64,
) -> Affine {
    let char_ascent = layout
        .lines()
        .next()
        .map_or(baseline_ascent, |l| l.metrics().baseline as f64);
    if stretch && is_cell_filler(s) {
        Affine::translate((x, y + baseline_ascent))
            * Affine::scale_non_uniform(1.0, ratio)
            * Affine::translate((0.0, -char_ascent))
    } else {
        Affine::translate((x, y + baseline_ascent - char_ascent))
    }
}

/// Terminal pane state (maintained separately from the terminal itself).
/// Scrolling is now owned by the ghostty engine, so there is no app-side
/// scroll offset anymore.
#[derive(Clone)]
pub struct TerminalPaneState {
    _private: (),
}

impl TerminalPaneState {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

/// Per-cell data collected for one viewport row.
struct CellDraw {
    x: usize,
    text: String,
    fg: Color,
    bg: Option<Color>,
    wide: bool,
    is_spacer: bool,
    selected: bool,
    underline: bool,
    strikethrough: bool,
}

/// Render a terminal onto a canvas with an offset
///
/// This function renders a terminal at a specific position within the canvas,
/// allowing multiple panes to be rendered in their layout positions.
pub fn render_terminal_with_offset(
    painter: &mut Painter<'_, Scene>,
    terminal: &Terminal,
    _pane_state: &TerminalPaneState,
    is_active: bool,
    size: Size,
    offset_x: f64,
    offset_y: f64,
    font_size: f32,
    fcx: &mut xilem::masonry::parley::FontContext,
    lcx: &mut xilem::masonry::parley::LayoutContext<xilem::masonry::core::BrushIndex>,
) {
    const MARGIN: f64 = PANE_MARGIN;

    ensure_symbol_font(fcx);

    // Background for this pane
    let rect = Rect::new(offset_x, offset_y, offset_x + size.width, offset_y + size.height);
    painter.fill(&rect, colors::CRUST).draw();

    // Build probe layouts to measure character advance dimensions.
    // Use a Latin-only probe for cell_w and a mixed Latin+CJK probe for
    // cell_h, because CJK fallback fonts (e.g. PingFang on macOS) often have
    // taller metrics than the primary monospace font, causing Chinese glyphs
    // to overflow the selection-highlight rect.
    let (cell_w, latin_line_h) = {
        let probe = "MMMMMMMMMM";
        let mut mb = lcx.ranged_builder(fcx, probe, 1.0, true);
        mb.push_default(StyleProperty::FontFamily(term_font_family()));
        mb.push_default(StyleProperty::FontSize(font_size));
        let mut ml = mb.build(probe);
        ml.break_all_lines(None);
        (ml.width() as f64 / 10.0, ml.height() as f64)
    };
    let (cell_h, baseline_ascent) = {
        let probe = "MMMMMMMMMM你好"; // Mixed to get the taller metrics
        let mut mb = lcx.ranged_builder(fcx, probe, 1.0, true);
        mb.push_default(StyleProperty::FontFamily(term_font_family()));
        mb.push_default(StyleProperty::FontSize(font_size));
        let mut ml = mb.build(probe);
        ml.break_all_lines(None);
        let h = ml.height() as f64;
        // Measure the ascent (distance from line top to baseline) from the
        // mixed-script probe, which reflects the taller CJK metrics.
        let ascent = ml.lines().next().map_or(h * 0.8, |l| l.metrics().baseline as f64);
        (h, ascent)
    };
    // Vertical stretch for cell-filling glyphs (box drawing / blocks) so they
    // stay contiguous when CJK metrics make cell_h taller than the font line.
    let stretch_ratio = (cell_h / latin_line_h).max(1.0);

    let max_displayable_rows = ((size.height - MARGIN).max(0.0) / cell_h).floor() as usize;

    // Grab the engine (UI thread owns it). Read scrollbar state before the
    // snapshot keeps the render state borrowed. Rebind to a plain `&mut Engine`
    // so disjoint field borrows (render/term/rows/cells) work through one
    // deref instead of re-borrowing the RefMut on every field access.
    let mut guard = terminal.engine();
    let (sb_total, sb_offset, sb_len) = match guard.term.scrollbar() {
        Ok(s) => (s.total, s.offset, s.len),
        Err(_) => (0, 0, 0),
    };
    let eng = &mut *guard;

    let Ok(snapshot) = eng.render.update(&eng.term) else {
        return;
    };
    let cols = snapshot.cols().unwrap_or(0) as usize;
    let rows = snapshot.rows().unwrap_or(0) as usize;
    let display_lines = rows.min(max_displayable_rows);

    let term_colors = snapshot.colors().ok();
    let default_fg = term_colors
        .as_ref()
        .map(|c| rgb_to_color(c.foreground))
        .unwrap_or(colors::TEXT);
    let cursor_visible = snapshot.cursor_visible().unwrap_or(false);
    let cursor_vp = snapshot.cursor_viewport().ok().flatten();
    let cursor_brush = snapshot
        .cursor_color()
        .ok()
        .flatten()
        .map(rgb_to_color)
        .or_else(|| term_colors.as_ref().and_then(|c| c.cursor).map(rgb_to_color))
        .unwrap_or(colors::TEXT);

    let Ok(mut row_iter) = eng.rows.update(&snapshot) else {
        return;
    };

    let mut row_idx: usize = 0;
    while let Some(row) = row_iter.next() {
        if row_idx >= display_lines {
            break;
        }
        let y = offset_y + row_idx as f64 * cell_h;
        let bg_row_y = y;

        let row_sel = row.selection().ok().flatten();
        let cursor_on_row = is_active
            && cursor_visible
            && cursor_vp.map(|c| c.y as usize == row_idx).unwrap_or(false);

        // Collect this row's cells.
        let mut cells: Vec<CellDraw> = Vec::with_capacity(cols);
        if let Ok(mut cell_iter) = eng.cells.update(row) {
            let mut x: usize = 0;
            while let Some(c) = cell_iter.next() {
                if x >= cols {
                    break;
                }
                let wide_kind = c
                    .raw_cell()
                    .ok()
                    .and_then(|r| r.wide().ok())
                    .unwrap_or(CellWide::Narrow);
                let is_spacer = matches!(wide_kind, CellWide::SpacerTail | CellWide::SpacerHead);
                let wide = matches!(wide_kind, CellWide::Wide);

                let has_text = c.raw_cell().ok().and_then(|r| r.has_text().ok()).unwrap_or(false);
                let graphemes = c.graphemes().unwrap_or_default();
                let text: String = graphemes.into_iter().filter(|ch| !ch.is_control()).collect();

                let fg = c.fg_color().ok().flatten().map(rgb_to_color).unwrap_or(default_fg);
                let bg = c.bg_color().ok().flatten().map(rgb_to_color);

                let mut underline = false;
                let mut strikethrough = false;
                if c.has_styling().unwrap_or(false) {
                    if let Ok(style) = c.style() {
                        underline = style.underline != Underline::None;
                        strikethrough = style.strikethrough;
                    }
                }

                let selected = row_sel
                    .map(|s| x as u16 >= s.start_x && x as u16 <= s.end_x)
                    .unwrap_or(false);

                cells.push(CellDraw {
                    x,
                    text: if has_text { text } else { String::new() },
                    fg,
                    bg,
                    wide,
                    is_spacer,
                    selected,
                    underline,
                    strikethrough,
                });
                x += 1;
            }
        }

        // Cell backgrounds (skip default bg and selected cells; wide cells
        // paint their trailing spacer too).
        for cell in &cells {
            if cell.is_spacer || cell.selected || cell.bg.is_none() {
                continue;
            }
            let bx = offset_x + cell.x as f64 * cell_w;
            let w = if cell.wide { cell_w * 2.0 } else { cell_w };
            let bg_color = cell.bg.unwrap();
            let cell_rect = Rect::new(bx, bg_row_y, bx + w, bg_row_y + cell_h);
            painter.fill(&cell_rect, bg_color).draw();
        }

        // Selection: one merged rect per row-local range to avoid seams.
        if cols > 0 {
            if let Some(sel) = row_sel {
                let start = (sel.start_x as usize).min(cols - 1);
                let end = (sel.end_x as usize).min(cols - 1);
                let bx = offset_x + start as f64 * cell_w;
                let ex = offset_x + (end + 1) as f64 * cell_w;
                let sel_rect = Rect::new(bx, bg_row_y, ex, bg_row_y + cell_h);
                painter.fill(&sel_rect, colors::BLUE).draw();
            }
        }

        // Underline / strikethrough
        for cell in &cells {
            if cell.is_spacer || (!cell.underline && !cell.strikethrough) {
                continue;
            }
            let ux = offset_x + cell.x as f64 * cell_w;
            let w = if cell.wide { cell_w * 2.0 } else { cell_w };
            if cell.underline {
                let ul_rect = Rect::new(ux, bg_row_y + cell_h - 2.0, ux + w, bg_row_y + cell_h);
                painter.fill(&ul_rect, cell.fg).draw();
            }
            if cell.strikethrough {
                let strike_rect = Rect::new(
                    ux,
                    bg_row_y + cell_h / 2.0 - 1.0,
                    ux + w,
                    bg_row_y + cell_h / 2.0 + 1.0,
                );
                painter.fill(&strike_rect, cell.fg).draw();
            }
        }

        // Cursor block (viewport-relative position from the snapshot).
        let cursor_cell_x = if cursor_on_row {
            cursor_vp.map(|c| {
                let mut cx = c.x as usize;
                if c.at_wide_tail && cx > 0 {
                    cx -= 1;
                }
                cx
            })
        } else {
            None
        };
        if let Some(cx) = cursor_cell_x {
            if cx < cols {
                let cursor_w = cells
                    .iter()
                    .find(|c| c.x == cx)
                    .map(|c| if c.wide { cell_w * 2.0 } else { cell_w })
                    .unwrap_or(cell_w);
                let cursor_x = offset_x + cx as f64 * cell_w;
                let cursor_rect = Rect::new(cursor_x, bg_row_y, cursor_x + cursor_w, bg_row_y + cell_h);
                painter.fill(&cursor_rect, cursor_brush).draw();
            }
        }

        // Render each character at its fixed cell position.
        // All characters share the same baseline (baseline_ascent from the
        // mixed-script probe) so Latin and CJK glyphs align correctly.
        for cell in &cells {
            if cell.is_spacer || cell.text.is_empty() {
                continue;
            }

            let is_cursor_cell = cursor_cell_x == Some(cell.x);
            let fg_color = if is_cursor_cell || cell.selected {
                colors::CRUST
            } else {
                cell.fg
            };
            let x = offset_x + cell.x as f64 * cell_w;

            let s = &cell.text;
            let mut builder = lcx.ranged_builder(fcx, s, 1.0, true);
            builder.push_default(StyleProperty::FontFamily(term_font_family()));
            builder.push_default(StyleProperty::FontSize(font_size));

            let stretch = stretch_ratio > 1.005 && is_cell_filler(s);
            if fg_color != colors::TEXT {
                let brush = Brush::Solid(fg_color);
                builder.push(
                    StyleProperty::Brush(xilem::masonry::core::BrushIndex(1)),
                    0..s.len(),
                );
                let mut layout = builder.build(s);
                layout.break_all_lines(None);
                let transform = glyph_transform(s, x, y, baseline_ascent, &layout, stretch, stretch_ratio);
                render_text(painter, transform, &layout, &[Brush::Solid(colors::TEXT), brush], true);
            } else {
                let mut layout = builder.build(s);
                layout.break_all_lines(None);
                let transform = glyph_transform(s, x, y, baseline_ascent, &layout, stretch, stretch_ratio);
                render_text(painter, transform, &layout, &[Brush::Solid(colors::TEXT)], true);
            }
        }

        row_idx += 1;
    }

    // Drop snapshot/iterators before releasing the engine borrow scope.
    drop(row_iter);

    // Scrollbar — only shown when there is scrollback beyond the viewport.
    if sb_total > sb_len {
        let track_width = 8.0;
        let track_x = offset_x + size.width - track_width;
        let track_rect = Rect::new(track_x, offset_y, track_x + track_width, offset_y + size.height - MARGIN);
        // Track background (semi-transparent)
        painter.fill(&track_rect, Color::from_rgba8(0x18, 0x18, 0x25, 0x80)).draw();

        // Thumb — size proportional to viewport/total, position from engine scrollbar offset
        {
            let scrollable_h = size.height - MARGIN;
            let viewport_ratio = (sb_len as f64 / sb_total as f64).min(1.0);
            let thumb_h = (scrollable_h * viewport_ratio).max(20.0);

            let max_scroll = sb_total.saturating_sub(sb_len) as f64;
            let scroll_progress = if max_scroll > 0.0 {
                (sb_offset as f64 / max_scroll).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let thumb_y = offset_y + (scrollable_h - thumb_h) * scroll_progress;
            let thumb_rect = Rect::new(track_x + 1.0, thumb_y, track_x + track_width - 1.0, thumb_y + thumb_h);
            painter.fill(&thumb_rect, colors::SURFACE1).draw();
        }
    }
}
