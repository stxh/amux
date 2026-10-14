/// Terminal canvas rendering - renders terminal grid onto a canvas surface
///
/// This module renders libghostty-vt render-state snapshots using Xilem's
/// canvas pattern. Supports per-character foreground/background coloring
/// using parley's ranged_builder.

use libghostty_vt::screen::CellWide;
use libghostty_vt::style::{RgbColor, Underline};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use xilem::masonry::core::render_text;
use xilem::masonry::imaging::{Painter, record::Scene};
use xilem::masonry::kurbo::{Affine, Rect, RoundedRect, Size, Stroke};
use xilem::masonry::parley::style::{FontFamily, FontWeight, StyleProperty};
use xilem::masonry::parley::{FontContext, Layout, LayoutContext};
use xilem::masonry::core::BrushIndex;

use xilem::masonry::peniko::{Brush, Color};

use crate::app::App;
use crate::pane_tree::Rect as HitRect;
use crate::terminal::Terminal;

/// Font fallback chain for terminal glyphs. Cascadia Mono (ships with Windows
/// Terminal) covers box-drawing, block elements, braille and check marks that
/// the generic monospace fallback (Consolas) lacks; the bundled Symbols Nerd
/// Font Mono supplies powerline/Nerd icons (U+E0B0.., U+F0..); Segoe UI
/// Symbol catches remaining dingbats.
///
/// CJK text fonts are listed explicitly rather than left to system fallback:
/// Chinese punctuation (，。：；！？（）《》「」etc.) is mostly `Common` script,
/// so fontique's script-keyed fallback does not route it to a CJK font and it
/// renders as tofu, even though Han ideographs (true `Han` script) resolve
/// fine. Naming real CJK families makes every CJK-range codepoint hit them
/// directly. Uninstalled names are ignored by fontique, so the Windows,
/// macOS and Linux families can all be listed together.
pub fn term_font_family() -> FontFamily<'static> {
    FontFamily::Source(Cow::Borrowed(
        "Cascadia Mono, Consolas, Symbols Nerd Font Mono, Segoe UI Symbol, \
         Microsoft YaHei, PingFang SC, Noto Sans CJK SC, monospace",
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
    // The new face can change which font resolves for a codepoint, so any
    // layout built before registration is no longer trustworthy.
    GLYPH_LAYOUTS.with(|cache| cache.borrow_mut().clear());
}

// Per-glyph layout cache.
//
// Terminals draw a few thousand cells per frame, and building a parley layout
// (bidi + itemisation + font fallback + shaping) for every single cell is what
// makes scrolling janky — the GPU itself is not the bottleneck. Terminal text
// repeats the same characters constantly, so layouts are cached by font size
// and text and only built once.
//
// The cached runs always carry brush index 1, which lets the caller hand the
// cell's foreground colour to `render_text` at draw time.
thread_local! {
    static GLYPH_LAYOUTS: RefCell<HashMap<u64, (Rc<str>, Rc<Layout<BrushIndex>>)>> =
        RefCell::new(HashMap::new());
}

/// Cached entries before the whole cache is dropped; a terminal's working set
/// of distinct glyphs is small, so this only guards against pathological input.
const GLYPH_CACHE_LIMIT: usize = 16_384;

// Cell metrics derived from font probes: `(font_size, cell_w, cell_h,
// latin_line_h, baseline_ascent)`.
//
// Probes build layouts too, so they are cached per font size instead of being
// rebuilt for every pane on every frame.
thread_local! {
    static CELL_METRICS: RefCell<Option<(f32, f64, f64, f64, f64)>> = const { RefCell::new(None) };
}

/// Hash of a glyph-layout key. The entry also keeps its text so a lookup can
/// reject the (astronomically unlikely) collision instead of mis-drawing it.
fn glyph_key(font_size: f32, text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    font_size.to_bits().hash(&mut hasher);
    text.hash(&mut hasher);
    hasher.finish()
}

/// Build (or reuse) the layout for one cell's text.
fn glyph_layout(
    lcx: &mut LayoutContext<BrushIndex>,
    fcx: &mut FontContext,
    text: &str,
    font_size: f32,
) -> Rc<Layout<BrushIndex>> {
    let key = glyph_key(font_size, text);
    GLYPH_LAYOUTS.with(|cache| {
        {
            let cache = cache.borrow();
            if let Some((cached_text, layout)) = cache.get(&key) {
                if &**cached_text == text {
                    return Rc::clone(layout);
                }
            }
        }
        let mut builder = lcx.ranged_builder(fcx, text, 1.0, true);
        builder.push_default(StyleProperty::FontFamily(term_font_family()));
        builder.push_default(StyleProperty::FontSize(font_size));
        builder.push(StyleProperty::Brush(BrushIndex(1)), 0..text.len());
        let mut layout = builder.build(text);
        layout.break_all_lines(None);
        let layout = Rc::new(layout);
        let mut cache = cache.borrow_mut();
        if cache.len() >= GLYPH_CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(key, (Rc::<str>::from(text), Rc::clone(&layout)));
        layout
    })
}

/// Cell metrics derived from font probes: `(cell_w, cell_h, line_height)`.
///
/// Probes build layouts too, so they are cached per font size instead of being
/// rebuilt for every pane on every frame. `line_height` is the Latin line
/// height used to detect the CJK-metric case; the baseline is derived from it.
fn cell_metrics_full(
    fcx: &mut FontContext,
    lcx: &mut LayoutContext<BrushIndex>,
    font_size: f32,
) -> (f64, f64, f64, f64) {
    CELL_METRICS.with(|slot| {
        if let Some(cached) = *slot.borrow() {
            if cached.0 == font_size {
                return (cached.1, cached.2, cached.3, cached.4);
            }
        }
        // Use a Latin-only probe for cell_w: it is the advance every monospace
        // glyph in the primary font shares, so cells line up.
        let (cell_w, latin_line_h) = {
            let probe = "MMMMMMMMMM";
            let mut mb = lcx.ranged_builder(fcx, probe, 1.0, true);
            mb.push_default(StyleProperty::FontFamily(term_font_family()));
            mb.push_default(StyleProperty::FontSize(font_size));
            let mut ml = mb.build(probe);
            ml.break_all_lines(None);
            (ml.width() as f64 / 10.0, ml.height() as f64)
        };
        // Use a mixed Latin+CJK probe for cell_h so fonts with taller CJK
        // metrics (e.g. PingFang on macOS) do not overflow the cell.
        let (cell_h, baseline_ascent) = {
            let probe = "MMMMMMMMMM你好";
            let mut mb = lcx.ranged_builder(fcx, probe, 1.0, true);
            mb.push_default(StyleProperty::FontFamily(term_font_family()));
            mb.push_default(StyleProperty::FontSize(font_size));
            let mut ml = mb.build(probe);
            ml.break_all_lines(None);
            let h = ml.height() as f64;
            let ascent = ml.lines().next().map_or(h * 0.8, |l| l.metrics().baseline as f64);
            (h, ascent)
        };
        let metrics = (font_size, cell_w, cell_h, latin_line_h, baseline_ascent);
        *slot.borrow_mut() = Some(metrics);
        (cell_w, cell_h, latin_line_h, baseline_ascent)
    })
}

/// Catppuccin Mocha color scheme
mod colors {
    use super::Color;
    pub const CRUST:     Color = Color::from_rgba8(0x11, 0x11, 0x1b, 0xff);
    pub const BASE:      Color = Color::from_rgba8(0x1e, 0x1e, 0x2e, 0xff);
    pub const SURFACE0:  Color = Color::from_rgba8(0x31, 0x32, 0x44, 0xff);
    pub const SURFACE1:  Color = Color::from_rgba8(0x45, 0x47, 0x5a, 0xff);
    pub const SURFACE2:  Color = Color::from_rgba8(0x58, 0x5b, 0x70, 0xff);
    pub const SUBTEXT:   Color = Color::from_rgba8(0xa6, 0xad, 0xc8, 0xff);
    pub const TEXT:      Color = Color::from_rgba8(0xc6, 0xd0, 0xf5, 0xff);
    pub const BLUE:      Color = Color::from_rgba8(0x89, 0xb4, 0xfa, 0xff);
}

/// Bottom inset below the last text row. Top/left/right are flush. Shared by
/// the renderer, the grid-resize math and click/selection coordinate mapping.
pub const PANE_MARGIN: f64 = 6.0;

/// Width of the scrollbar track, in pixels. Shared by the renderer and the
/// mouse hit-testing so both agree on the draggable area.
const SCROLLBAR_WIDTH: f64 = 8.0;

/// Geometry of one pane's scrollbar for the current frame.
///
/// Rebuilt on every render pass so `App` can hit-test thumb presses and drags
/// against exactly what is on screen.
#[derive(Clone, Copy, Debug)]
pub struct ScrollbarGeometry {
    pub pane_id: crate::pane_tree::PaneId,
    /// Full track area (the wider of the two shapes users can grab).
    pub track_x: f64,
    pub track_y: f64,
    pub track_h: f64,
    /// Thumb position inside the track.
    pub thumb_y: f64,
    pub thumb_h: f64,
    /// Scrollable rows above the viewport (total - visible), in lines.
    pub max_scroll: f64,
}

impl ScrollbarGeometry {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.track_x
            && x <= self.track_x + SCROLLBAR_WIDTH
            && y >= self.track_y
            && y <= self.track_y + self.track_h
    }

    pub fn thumb_contains(&self, y: f64) -> bool {
        y >= self.thumb_y && y <= self.thumb_y + self.thumb_h
    }
}

/// Compute the scrollbar geometry for a pane, or `None` when there is no
/// scrollback to show (the scrollbar is only drawn in that case).
pub fn scrollbar_geometry(
    pane_id: crate::pane_tree::PaneId,
    offset_x: f64,
    offset_y: f64,
    size: Size,
    total: usize,
    offset: usize,
    len: usize,
) -> Option<ScrollbarGeometry> {
    if total <= len {
        return None;
    }
    let track_h = (size.height - PANE_MARGIN).max(1.0);
    let viewport_ratio = (len as f64 / total as f64).min(1.0);
    let thumb_h = (track_h * viewport_ratio).max(20.0).min(track_h);
    let max_scroll = (total - len) as f64;
    let progress = (offset as f64 / max_scroll).clamp(0.0, 1.0);
    Some(ScrollbarGeometry {
        pane_id,
        track_x: offset_x + size.width - SCROLLBAR_WIDTH,
        track_y: offset_y,
        track_h,
        thumb_y: offset_y + (track_h - thumb_h) * progress,
        thumb_h,
        max_scroll,
    })
}

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
///
/// Returns the scrollbar geometry of this frame (when the pane has scrollback),
/// which the caller caches for mouse hit testing.
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
    pane_id: crate::pane_tree::PaneId,
) -> Option<ScrollbarGeometry> {
    const MARGIN: f64 = PANE_MARGIN;

    ensure_symbol_font(fcx);

    // Background for this pane
    let rect = Rect::new(offset_x, offset_y, offset_x + size.width, offset_y + size.height);
    painter.fill(&rect, colors::CRUST).draw();

    // Cell metrics come from font probes; caching them keeps every pane from
    // re-running the probe layouts on every frame.
    let (cell_w, cell_h, latin_line_h, baseline_ascent) =
        cell_metrics_full(fcx, lcx, font_size);
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

    // Geometry first: it only needs the scrollbar state, so the hit-test rect
    // stays valid even if the render snapshot below fails.
    let scrollbar = scrollbar_geometry(
        pane_id,
        offset_x,
        offset_y,
        size,
        sb_total as usize,
        sb_offset as usize,
        sb_len as usize,
    );

    let Ok(snapshot) = eng.render.update(&eng.term) else {
        return scrollbar;
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
        return scrollbar;
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
            let stretch = stretch_ratio > 1.005 && is_cell_filler(s);
            // Cached layout: rebuilding a layout per cell is what made scrolling
            // janky, and the cached run always carries brush index 1 so the
            // cell's foreground can be supplied here.
            let layout = glyph_layout(lcx, fcx, s, font_size);
            let transform = glyph_transform(s, x, y, baseline_ascent, &layout, stretch, stretch_ratio);
            render_text(
                painter,
                transform,
                &layout,
                &[Brush::Solid(colors::TEXT), Brush::Solid(fg_color)],
                true,
            );
        }

        row_idx += 1;
    }

    // Drop snapshot/iterators before releasing the engine borrow scope.
    drop(row_iter);

    // Scrollbar — drawn from the shared geometry so what is painted is exactly
    // what `App` hit-tests for clicks and drags.
    if let Some(sb) = scrollbar {
        // Track background (semi-transparent)
        let track_rect = Rect::new(sb.track_x, sb.track_y, sb.track_x + SCROLLBAR_WIDTH, sb.track_y + sb.track_h);
        painter.fill(&track_rect, Color::from_rgba8(0x18, 0x18, 0x25, 0x80)).draw();

        // Thumb — size proportional to viewport/total, position from engine scrollbar offset
        let thumb_rect = Rect::new(sb.track_x + 1.0, sb.thumb_y, sb.track_x + SCROLLBAR_WIDTH - 1.0, sb.thumb_y + sb.thumb_h);
        painter.fill(&thumb_rect, colors::SURFACE1).draw();
    }

    scrollbar
}

// ─── System settings: gear entry button & modal overlay ───

/// Actions exposed by the system settings overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsAction {
    FontMinus,
    FontPlus,
    ToggleSidebar,
    ToggleRightSidebar,
    ToggleSingleInstance,
    OpenConfigDir,
    Close,
    Quit,
}

/// Draw one line of UI text; returns its advance width.
fn draw_ui_text(
    painter: &mut Painter<'_, Scene>,
    fcx: &mut FontContext,
    lcx: &mut LayoutContext<xilem::masonry::core::BrushIndex>,
    text: &str,
    x: f64,
    baseline_y: f64,
    size: f32,
    color: Color,
    bold: bool,
) -> f64 {
    let mut builder = lcx.ranged_builder(fcx, text, 1.0, true);
    builder.push_default(StyleProperty::FontFamily(term_font_family()));
    builder.push_default(StyleProperty::FontSize(size));
    if bold {
        builder.push_default(StyleProperty::FontWeight(FontWeight::BOLD));
    }
    builder.push(StyleProperty::Brush(xilem::masonry::core::BrushIndex(1)), 0..text.len());
    let mut layout = builder.build(text);
    layout.break_all_lines(None);

    let ascent = layout
        .lines()
        .next()
        .map_or(size as f64 * 0.8, |l| l.metrics().baseline as f64);
    render_text(
        painter,
        Affine::translate((x, baseline_y - ascent)),
        &layout,
        &[Brush::Solid(colors::TEXT), Brush::Solid(color)],
        true,
    );
    layout.width() as f64
}

/// Horizontally centred variant of [`draw_ui_text`].
fn draw_ui_text_centered(
    painter: &mut Painter<'_, Scene>,
    fcx: &mut FontContext,
    lcx: &mut LayoutContext<xilem::masonry::core::BrushIndex>,
    text: &str,
    center_x: f64,
    baseline_y: f64,
    size: f32,
    color: Color,
    bold: bool,
) -> f64 {
    // Measure first, then draw at the computed origin.
    let width = measure_text(fcx, lcx, text, size, bold);
    draw_ui_text(painter, fcx, lcx, text, center_x - width / 2.0, baseline_y, size, color, bold)
}

/// Measure the advance width of `text` without painting it.
fn measure_text(
    fcx: &mut FontContext,
    lcx: &mut LayoutContext<xilem::masonry::core::BrushIndex>,
    text: &str,
    size: f32,
    bold: bool,
) -> f64 {
    let mut builder = lcx.ranged_builder(fcx, text, 1.0, true);
    builder.push_default(StyleProperty::FontFamily(term_font_family()));
    builder.push_default(StyleProperty::FontSize(size));
    if bold {
        builder.push_default(StyleProperty::FontWeight(FontWeight::BOLD));
    }
    let mut layout = builder.build(text);
    layout.break_all_lines(None);
    layout.width() as f64
}

/// Expand a hit rect by `margin` on all sides for easier clicking.
fn inflate(rect: &HitRect, margin: f64) -> HitRect {
    HitRect::new(
        rect.x - margin,
        rect.y - margin,
        rect.width + margin * 2.0,
        rect.height + margin * 2.0,
    )
}

/// Paint a clickable chip and return the rect its text should be centred on.
fn draw_chip(
    painter: &mut Painter<'_, Scene>,
    rect: &HitRect,
    hovered: bool,
) {
    let chip = RoundedRect::new(rect.x, rect.y, rect.x + rect.width, rect.y + rect.height, 6.0);
    let fill = if hovered { colors::SURFACE2 } else { colors::SURFACE0 };
    painter.fill(&chip, fill).draw();
    painter.stroke(&chip, &Stroke::new(1.0), colors::SURFACE1).draw();
}

/// One row of the settings overlay.
struct SettingsRow {
    label: &'static str,
    control: SettingsControl,
}

/// The interactive control rendered on the right side of a settings row.
enum SettingsControl {
    /// Font size stepper: minus / value / plus.
    FontStepper,
    /// Toggle whose label reflects the current state.
    Toggle { on: bool, action: SettingsAction },
    /// Plain action button.
    Button { text: &'static str, action: SettingsAction },
}

/// Draw the settings modal over the whole terminal area.
///
/// Interactive rects are collected into `app.settings_items` and the panel
/// bounds into `app.settings_panel`, so `App::handle_mouse_down` can hit-test
/// them on the next click.
pub fn render_settings_overlay(
    painter: &mut Painter<'_, Scene>,
    app: &mut App,
    area: Size,
    fcx: &mut FontContext,
    lcx: &mut LayoutContext<xilem::masonry::core::BrushIndex>,
) {
    const PANEL_W: f64 = 400.0;
    const PAD: f64 = 18.0;
    const TITLE_H: f64 = 36.0;
    const ROW_H: f64 = 34.0;
    const FOOTER_H: f64 = 30.0;
    const CHIP_H: f64 = 22.0;

    let rows = [
        SettingsRow {
            label: "字体大小",
            control: SettingsControl::FontStepper,
        },
        SettingsRow {
            label: "左侧边栏",
            control: SettingsControl::Toggle { on: app.show_sidebar, action: SettingsAction::ToggleSidebar },
        },
        SettingsRow {
            label: "右侧状态栏",
            control: SettingsControl::Toggle {
                on: app.show_right_sidebar,
                action: SettingsAction::ToggleRightSidebar,
            },
        },
        SettingsRow {
            label: "单实例模式",
            control: SettingsControl::Toggle {
                on: app.user_config.single_instance,
                action: SettingsAction::ToggleSingleInstance,
            },
        },
        SettingsRow {
            label: "配置目录",
            control: SettingsControl::Button { text: "打开", action: SettingsAction::OpenConfigDir },
        },
        SettingsRow {
            label: "退出应用",
            control: SettingsControl::Button { text: "退出", action: SettingsAction::Quit },
        },
    ];

    app.settings_items.clear();
    let hover = app.hover_pos;

    let panel_h = PAD + TITLE_H + rows.len() as f64 * ROW_H + FOOTER_H + PAD;
    let px = ((area.width - PANEL_W) / 2.0).max(8.0);
    let py = ((area.height - panel_h) / 2.0).max(8.0);
    app.settings_panel = Some(HitRect::new(px, py, PANEL_W, panel_h));

    // Scrim: dim the terminal behind the modal.
    painter
        .fill(&Rect::new(0.0, 0.0, area.width, area.height), Color::from_rgba8(0x11, 0x11, 0x1b, 0xb4))
        .draw();

    // Panel background + border.
    let panel = RoundedRect::new(px, py, px + PANEL_W, py + panel_h, 10.0);
    painter.fill(&panel, colors::BASE).draw();
    painter.stroke(&panel, &Stroke::new(1.0), colors::SURFACE1).draw();

    let content_left = px + PAD;
    let content_right = px + PANEL_W - PAD;

    // Header: title plus a close chip on the right.
    let title_baseline = py + PAD + 17.0;
    draw_ui_text(painter, fcx, lcx, "系统设置", content_left, title_baseline, 15.0, colors::TEXT, true);
    let title_w = measure_text(fcx, lcx, "系统设置", 15.0, true);
    draw_ui_text(
        painter,
        fcx,
        lcx,
        "SYSTEM SETTINGS",
        content_left + title_w + 10.0,
        title_baseline,
        10.0,
        colors::SUBTEXT,
        false,
    );

    let close_w = 28.0;
    let close_rect = HitRect::new(content_right - close_w, py + PAD, close_w, CHIP_H);
    let close_hover = hover.map_or(false, |(hx, hy)| {
        hx >= close_rect.x && hx <= close_rect.x + close_rect.width
            && hy >= close_rect.y && hy <= close_rect.y + close_rect.height
    });
    draw_chip(painter, &close_rect, close_hover);
    draw_ui_text_centered(
        painter,
        fcx,
        lcx,
        "×",
        close_rect.x + close_rect.width / 2.0,
        py + PAD + CHIP_H / 2.0 + 4.5,
        13.0,
        colors::TEXT,
        false,
    );
    app.settings_items.push((SettingsAction::Close, inflate(&close_rect, 2.0)));

    // Rows.
    let mut y = py + PAD + TITLE_H;
    for row in rows {
        let row_rect = HitRect::new(content_left, y, content_right - content_left, ROW_H);
        let row_center_y = y + ROW_H / 2.0;
        draw_ui_text(
            painter,
            fcx,
            lcx,
            row.label,
            row_rect.x + 10.0,
            row_center_y + 4.5,
            13.0,
            colors::TEXT,
            false,
        );

        match row.control {
            SettingsControl::FontStepper => {
                let chip_w = 26.0;
                let value_w = 44.0;
                let gap = 6.0;
                let x0 = content_right - (chip_w * 2.0 + value_w + gap * 2.0);
                let chip_y = row_center_y - CHIP_H / 2.0;

                let minus_rect = HitRect::new(x0, chip_y, chip_w, CHIP_H);
                let minus_hover = hover.map_or(false, |(hx, hy)| {
                    hx >= minus_rect.x && hx <= minus_rect.x + minus_rect.width
                        && hy >= minus_rect.y && hy <= minus_rect.y + minus_rect.height
                });
                draw_chip(painter, &minus_rect, minus_hover);
                draw_ui_text_centered(
                    painter, fcx, lcx, "−", minus_rect.x + chip_w / 2.0, chip_y + CHIP_H / 2.0 + 4.5,
                    13.0, colors::TEXT, false,
                );
                app.settings_items.push((SettingsAction::FontMinus, inflate(&minus_rect, 2.0)));

                let value = format!("{}", app.font_size.round() as i64);
                draw_ui_text_centered(
                    painter, fcx, lcx, &value, x0 + chip_w + gap + value_w / 2.0,
                    chip_y + CHIP_H / 2.0 + 4.5, 13.0, colors::BLUE, true,
                );

                let plus_x = x0 + chip_w + gap + value_w + gap;
                let plus_rect = HitRect::new(plus_x, chip_y, chip_w, CHIP_H);
                let plus_hover = hover.map_or(false, |(hx, hy)| {
                    hx >= plus_rect.x && hx <= plus_rect.x + plus_rect.width
                        && hy >= plus_rect.y && hy <= plus_rect.y + plus_rect.height
                });
                draw_chip(painter, &plus_rect, plus_hover);
                draw_ui_text_centered(
                    painter, fcx, lcx, "+", plus_rect.x + chip_w / 2.0, chip_y + CHIP_H / 2.0 + 4.5,
                    13.0, colors::TEXT, false,
                );
                app.settings_items.push((SettingsAction::FontPlus, inflate(&plus_rect, 2.0)));
            }
            SettingsControl::Toggle { on, action } => {
                let chip_w = if on { 52.0 } else { 44.0 };
                let chip_rect = HitRect::new(content_right - chip_w, row_center_y - CHIP_H / 2.0, chip_w, CHIP_H);
                let chip_hover = hover.map_or(false, |(hx, hy)| {
                    hx >= chip_rect.x && hx <= chip_rect.x + chip_rect.width
                        && hy >= chip_rect.y && hy <= chip_rect.y + chip_rect.height
                });
                draw_chip(painter, &chip_rect, chip_hover);
                let text = if on { "开启" } else { "关闭" };
                draw_ui_text_centered(
                    painter, fcx, lcx, text,
                    chip_rect.x + chip_rect.width / 2.0, row_center_y + 4.5, 12.0,
                    if on { colors::BLUE } else { colors::SUBTEXT }, false,
                );
                app.settings_items.push((action, inflate(&chip_rect, 2.0)));
            }
            SettingsControl::Button { text, action } => {
                let chip_w = 56.0;
                let chip_rect = HitRect::new(content_right - chip_w, row_center_y - CHIP_H / 2.0, chip_w, CHIP_H);
                let chip_hover = hover.map_or(false, |(hx, hy)| {
                    hx >= chip_rect.x && hx <= chip_rect.x + chip_rect.width
                        && hy >= chip_rect.y && hy <= chip_rect.y + chip_rect.height
                });
                draw_chip(painter, &chip_rect, chip_hover);
                draw_ui_text_centered(
                    painter, fcx, lcx, text,
                    chip_rect.x + chip_rect.width / 2.0, row_center_y + 4.5, 12.0,
                    colors::TEXT, false,
                );
                app.settings_items.push((action, inflate(&chip_rect, 2.0)));
            }
        }

        y += ROW_H;
    }

    // Footer hint.
    draw_ui_text(
        painter,
        fcx,
        lcx,
        "Esc 关闭 · 点击面板外部关闭",
        content_left,
        y + FOOTER_H / 2.0 + 4.0,
        11.0,
        colors::SUBTEXT,
        false,
    );
}
