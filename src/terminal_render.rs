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
use xilem::masonry::imaging::{NormalizedCoord, Painter, record::Glyph as RecordGlyph, record::Scene};
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
    UI_TEXT.with(|cache| cache.borrow_mut().clear());
}

// Per-glyph cache.
//
// Terminals draw a few thousand cells per frame, and building a parley layout
// (bidi + itemisation + font fallback + shaping) for every single cell is what
// makes scrolling janky — the GPU itself is not the bottleneck. Terminal text
// repeats the same characters constantly, so everything that only depends on
// the text is computed once and cached by font size and text:
//
//   * the layout itself (for the per-cell `render_text` path),
//   * the glyph ids and their offsets inside the cell,
//   * the font and shaping identity a batched glyph run has to match.
//
// The cached runs always carry brush index 1, which lets the caller hand the
// cell's foreground colour to `render_text` at draw time.
// type CachedLayouts = ... is declared next to the overlay cache below; both
// map a cached text to its layout.
thread_local! {
    static GLYPH_LAYOUTS: RefCell<HashMap<u64, (Rc<str>, Rc<GlyphEntry>)>> =
        RefCell::new(HashMap::new());
}

/// One glyph of a cell, positioned relative to the cell's left edge and to the
/// row's shared baseline.
#[derive(Clone, Copy)]
struct CellGlyph {
    id: u32,
    dx: f32,
    dy: f32,
}

/// Everything the renderer needs to draw one cell's text.
struct GlyphEntry {
    layout: Rc<Layout<BrushIndex>>,
    /// Glyphs positioned relative to the cell origin; empty when the text has
    /// no drawable run.
    glyphs: Vec<CellGlyph>,
    /// Font identity: a batch may only absorb this entry's glyphs when its
    /// font, size and variable-axis coordinates match. `None` when the text
    /// has no drawable run.
    font: Option<xilem::masonry::peniko::FontData>,
    font_size: f32,
    coords: Vec<NormalizedCoord>,
    /// Set when the layout cannot be drawn inside a batched run — several runs,
    /// a synthesised skew, or nothing drawable at all.
    per_cell: bool,
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
///
/// This is an FxHash-style mix rather than the default SipHash: the cache is
/// probed once per drawn glyph, i.e. thousands of times per frame, and the key
/// is never exposed to untrusted input (the stored text guards collisions).
fn glyph_key(font_size: f32, text: &str) -> u64 {
    const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

    let mut hash = (font_size.to_bits() as u64).wrapping_mul(SEED) ^ SEED;
    let mut bytes = text.as_bytes();
    while let Some((chunk, rest)) = bytes.split_first_chunk::<8>() {
        hash = (hash ^ u64::from_le_bytes(*chunk))
            .rotate_left(5)
            .wrapping_mul(SEED);
        bytes = rest;
    }
    if !bytes.is_empty() {
        let mut tail = [0u8; 8];
        tail[..bytes.len()].copy_from_slice(bytes);
        hash = (hash ^ u64::from_le_bytes(tail))
            .rotate_left(5)
            .wrapping_mul(SEED);
    }
    hash
}

/// Build (or reuse) everything needed to draw one cell's text.
fn glyph_entry(
    lcx: &mut LayoutContext<BrushIndex>,
    fcx: &mut FontContext,
    text: &str,
    font_size: f32,
) -> Rc<GlyphEntry> {
    let key = glyph_key(font_size, text);
    GLYPH_LAYOUTS.with(|cache| {
        {
            let cache = cache.borrow();
            if let Some((cached_text, entry)) = cache.get(&key) {
                if &**cached_text == text {
                    return Rc::clone(entry);
                }
            }
        }
        let mut builder = lcx.ranged_builder(fcx, text, 1.0, true);
        builder.push_default(StyleProperty::FontFamily(term_font_family()));
        builder.push_default(StyleProperty::FontSize(font_size));
        builder.push(StyleProperty::Brush(BrushIndex(1)), 0..text.len());
        let mut layout = builder.build(text);
        layout.break_all_lines(None);
        let entry = Rc::new(glyph_entry_of(Rc::new(layout)));
        let mut cache = cache.borrow_mut();
        if cache.len() >= GLYPH_CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(key, (Rc::<str>::from(text), Rc::clone(&entry)));
        entry
    })
}

/// Pull the drawable glyphs and shaping identity out of a freshly built layout.
///
/// A layout that shapes into several runs, or that needs a synthesised skew,
/// cannot share a batched run and is marked for the per-cell path.
fn glyph_entry_of(layout: Rc<Layout<BrushIndex>>) -> GlyphEntry {
    let mut entry = GlyphEntry {
        layout: Rc::clone(&layout),
        glyphs: Vec::new(),
        font: None,
        font_size: 0.0,
        coords: Vec::new(),
        per_cell: true,
    };

    let mut runs = 0usize;
    for line in layout.lines() {
        for item in line.items() {
            let xilem::masonry::parley::layout::PositionedLayoutItem::GlyphRun(run) = item else {
                continue;
            };
            runs += 1;
            if runs > 1 || run.run().synthesis().skew().is_some() {
                return entry;
            }
            // The row's baseline is shared by every cell, so a glyph is stored
            // relative to it: `run.baseline()` minus the layout's own baseline.
            let baseline_shift = -line.metrics().baseline;
            let mut pen_x = run.offset();
            let pen_y = run.baseline() + baseline_shift;
            for glyph in run.glyphs() {
                entry.glyphs.push(CellGlyph {
                    id: glyph.id,
                    dx: pen_x + glyph.x,
                    dy: pen_y + glyph.y,
                });
                pen_x += glyph.advance;
            }
            entry.font = Some(run.run().font().clone());
            entry.font_size = run.run().font_size();
            entry.coords = run.run().normalized_coords().to_vec();
        }
    }

    entry.per_cell = runs != 1;
    entry
}

/// Cell metrics derived from font probes: `(cell_w, cell_h, line_height)`.
///
/// Probes build layouts too, so they are cached per font size instead of being
/// rebuilt for every pane on every frame. `line_height` is the Latin line
/// height used to detect the CJK-metric case; the baseline is derived from it.
pub(crate) fn cell_metrics_full(
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

/// Snap a row's vertical span to device pixels.
///
/// Cell heights are fractional once CJK metrics are in play, so filling rows
/// with their raw float bounds leaves 1px seams between every few rows (the
/// dark pane background shows through as black lines). Flooring the top and
/// ceiling the bottom makes consecutive rows abut or overlap instead.
fn row_span(y: f64, cell_h: f64) -> (f64, f64) {
    (y.floor(), (y + cell_h).ceil())
}

// ─── Frame-time profile (AMUX_FRAME_LOG) ───
//
// Recording the scene is the expensive half of a frame and the terminal's own
// scanning is the rest, so knowing how long each phase takes is the difference
// between guessing and knowing whether scrolling is GPU-bound or CPU-bound.
// Profiling is off unless `AMUX_FRAME_LOG` is set, and reports every 120 frames.

/// Whether `AMUX_FRAME_LOG` is set (resolved once).
pub(crate) fn profiling_enabled() -> bool {
    use std::sync::atomic::{AtomicU8, Ordering};
    static ENABLED: AtomicU8 = AtomicU8::new(0);
    match ENABLED.load(Ordering::Relaxed) {
        0 => {
            let on = std::env::var_os("AMUX_FRAME_LOG").is_some() as u8 + 1;
            ENABLED.store(on, Ordering::Relaxed);
            on == 2
        }
        2 => true,
        _ => false,
    }
}

/// Accumulates one frame's totals across all panes of that frame.
#[derive(Clone, Copy, Default)]
struct Profile {
    frames: u64,
    frame_ns: u128,
    max_frame_ns: u128,
    snapshot_ns: u128,
    scan_ns: u128,
    glyph_ns: u128,
    cells: u64,
    glyphs: u64,
    /// Glyph runs recorded: one per batch, not one per character.
    batches: u64,
}

thread_local! {
    static PROFILE: RefCell<Profile> = const { RefCell::new(Profile {
        frames: 0,
        frame_ns: 0,
        max_frame_ns: 0,
        snapshot_ns: 0,
        scan_ns: 0,
        glyph_ns: 0,
        cells: 0,
        glyphs: 0,
        batches: 0,
    }) };
}

const PROFILE_REPORT_EVERY: u64 = 120;

/// Fold one pane's pass into the frame profile.
fn profile_grid(stats: GridStats) {
    if !profiling_enabled() {
        return;
    }
    PROFILE.with(|p| {
        let mut p = p.borrow_mut();
        p.snapshot_ns += stats.snapshot_ns;
        p.scan_ns += stats.scan_ns;
        p.glyph_ns += stats.glyph_ns;
        p.cells += stats.cells;
        p.glyphs += stats.glyphs;
        p.batches += stats.batches;
    });
}

/// Close the frame: record its total and report once the window is full.
pub(crate) fn profile_frame(frame_ns: u128) {
    if !profiling_enabled() {
        return;
    }
    PROFILE.with(|p| {
        let mut p = p.borrow_mut();
        p.frames += 1;
        p.frame_ns += frame_ns;
        p.max_frame_ns = p.max_frame_ns.max(frame_ns);
        if p.frames < PROFILE_REPORT_EVERY {
            return;
        }
        let n = p.frames as f64;
        let ms = |ns: u128| ns as f64 / 1e6 / n;
        eprintln!(
            "[amux frame] n={} avg={:.2}ms max={:.2}ms | snapshot={:.2}ms scan={:.2}ms \
             glyphs={:.2}ms | {:.0} cells {:.0} glyphs in {:.0} runs",
            p.frames,
            ms(p.frame_ns),
            p.max_frame_ns as f64 / 1e6,
            ms(p.snapshot_ns),
            ms(p.scan_ns),
            ms(p.glyph_ns),
            p.cells as f64 / n,
            p.glyphs as f64 / n,
            p.batches as f64 / n,
        );
        *p = Profile {
            frames: 0,
            frame_ns: 0,
            max_frame_ns: 0,
            snapshot_ns: 0,
            scan_ns: 0,
            glyph_ns: 0,
            cells: 0,
            glyphs: 0,
            batches: 0,
        };
    });
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
    /// Byte range of this cell's text inside the row text arena.
    text: std::ops::Range<usize>,
    fg: Color,
    bg: Option<Color>,
    wide: bool,
    is_spacer: bool,
    selected: bool,
    underline: bool,
    strikethrough: bool,
}

/// Reusable buffers for scanning a pane's rows.
///
/// Every query against a cell of the render snapshot crosses the FFI boundary,
/// so the per-frame work is kept to the fewest calls that still carry all the
/// information the renderer needs. These buffers are reused across rows and
/// frames: cell texts are appended back to back into `text` and each cell only
/// records its own slice, which removes the per-cell `String`/`Vec<char>`
/// allocation the renderer used to make for every cell of every frame.
struct RowScratch {
    cells: Vec<CellDraw>,
    /// Every cell's text of the current row, back to back.
    text: String,
    /// The engine's UTF-8 encoder *replaces* the buffer's contents rather than
    /// appending, so each cell's text lands here first and is then copied into
    /// the row arena. Cleared, not reallocated, so it costs nothing per cell.
    glyph: String,
}

impl RowScratch {
    const fn new() -> Self {
        Self {
            cells: Vec::new(),
            text: String::new(),
            glyph: String::new(),
        }
    }
}

thread_local! {
    static ROW_SCRATCH: RefCell<RowScratch> = const { RefCell::new(RowScratch::new()) };
}

/// A half-open run of adjacent cells that share a fill colour.
type FillRun = Option<(f64, f64, Color)>;

/// Emit a pending run as a single fill, if one is open.
fn flush_run(painter: &mut Painter<'_, Scene>, run: &mut FillRun, top: f64, bottom: f64) {
    if let Some((x0, x1, color)) = run.take() {
        painter.fill(&Rect::new(x0, top, x1, bottom), color).draw();
    }
}

/// Whether a cell's text contains anything that never renders as a glyph.
///
/// C0 controls, DEL and the C1 range are all `char::is_control`, so this is
/// the same rule as before — just asked without allocating.
fn has_control_char(text: &str) -> bool {
    text.chars().any(char::is_control)
}

// ─── Batched glyph runs ───
//
// A row is typically a long stretch of one colour, but drawing cell by cell
// records one glyph run per character — and each of those runs owns a `Vec` of
// glyphs. A 120x40 grid of text is therefore ~2 300 heap allocations and ~2 300
// scene draw ops per frame, which is where the remaining frame time goes.
// Adjacent cells that resolve to the same font and colour are emitted as one
// glyph run instead, which leaves the picture identical while collapsing a row
// into a couple of ops.

/// Cells accumulated into one glyph run, positioned in absolute pixels.
struct GlyphBatch {
    font: xilem::masonry::peniko::FontData,
    coords: Vec<NormalizedCoord>,
    font_size: f32,
    brush: Color,
    glyphs: Vec<RecordGlyph>,
}

impl GlyphBatch {
    fn flush(self, painter: &mut Painter<'_, Scene>, transform: Affine) {
        painter
            .glyphs(&self.font, self.brush)
            .hint(true)
            .transform(transform)
            .font_size(self.font_size)
            .normalized_coords(&self.coords)
            .draw(&peniko::Style::Fill(peniko::Fill::NonZero), &self.glyphs);
    }
}

/// Draw a batched run and clear the slot, if one is open.
fn flush_glyph_batch(painter: &mut Painter<'_, Scene>, batch: &mut Option<GlyphBatch>, transform: Affine) {
    if let Some(batch) = batch.take() {
        batch.flush(painter, transform);
    }
}

/// Add one cell's glyphs to the open batch, opening a new run when the cell
/// changes font, colour or shaping parameters.
///
/// Returns `false` when the cell cannot join a batch — a layout marked
/// `per_cell`, i.e. several runs, a synthesised skew or nothing drawable — and
/// the caller then draws it with `render_text` on its own.
fn batch_entry(
    painter: &mut Painter<'_, Scene>,
    transform: Affine,
    entry: &GlyphEntry,
    cell_x: f32,
    brush: Color,
    batch: &mut Option<GlyphBatch>,
    stats: &mut GridStats,
) -> bool {
    if entry.per_cell {
        return false;
    }
    let Some(font) = &entry.font else {
        return false;
    };
    let joins = batch.as_ref().is_some_and(|b| {
        b.font == *font
            && b.brush == brush
            && b.font_size == entry.font_size
            && b.coords == entry.coords
    });
    if !joins {
        // The open run has to be recorded before it is replaced.
        flush_glyph_batch(painter, batch, transform);
        *batch = Some(GlyphBatch {
            font: font.clone(),
            coords: entry.coords.clone(),
            font_size: entry.font_size,
            brush,
            glyphs: Vec::new(),
        });
        stats.batches += 1;
    }

    // The batch's transform carries the row and the shared baseline; a glyph
    // only adds its cell x.
    let batch = batch.as_mut().expect("batch opened above");
    batch.glyphs.reserve(entry.glyphs.len());
    for glyph in &entry.glyphs {
        batch.glyphs.push(RecordGlyph {
            id: glyph.id,
            x: cell_x + glyph.dx,
            y: glyph.dy,
        });
    }
    true
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
    ensure_symbol_font(fcx);

    // Background for this pane
    let rect = Rect::new(offset_x, offset_y, offset_x + size.width, offset_y + size.height);
    painter.fill(&rect, colors::CRUST).draw();

    // Cell metrics come from font probes; caching them keeps every pane from
    // re-running the probe layouts on every frame.
    let (cell_w, cell_h, latin_line_h, baseline_ascent) =
        cell_metrics_full(fcx, lcx, font_size);

    // Grab the engine (UI thread owns it).
    let mut guard = terminal.engine();
    let (scrollbar, stats) = render_grid(
        painter,
        &mut guard,
        pane_id,
        is_active,
        size,
        offset_x,
        offset_y,
        font_size,
        cell_w,
        cell_h,
        latin_line_h,
        baseline_ascent,
        fcx,
        lcx,
    );
    drop(guard);
    profile_grid(stats);

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

/// Counters for one pane's render pass, folded into the frame profile.
#[derive(Clone, Copy, Default)]
struct GridStats {
    cells: u64,
    glyphs: u64,
    batches: u64,
    scan_ns: u128,
    glyph_ns: u128,
    snapshot_ns: u128,
}

/// Scan one pane's snapshot and record it into the scene.
///
/// Split out of [`render_terminal_with_offset`] so the engine borrow, the row
/// scratch and the phase timers live in one place. Returns the frame's
/// scrollbar geometry (computed before the snapshot, so hit testing keeps
/// working even when the snapshot fails) plus the per-frame counters.
#[allow(clippy::too_many_arguments)]
fn render_grid(
    painter: &mut Painter<'_, Scene>,
    eng: &mut crate::terminal::Engine,
    pane_id: crate::pane_tree::PaneId,
    is_active: bool,
    size: Size,
    offset_x: f64,
    offset_y: f64,
    font_size: f32,
    cell_w: f64,
    cell_h: f64,
    latin_line_h: f64,
    baseline_ascent: f64,
    fcx: &mut FontContext,
    lcx: &mut LayoutContext<BrushIndex>,
) -> (Option<ScrollbarGeometry>, GridStats) {
    const MARGIN: f64 = PANE_MARGIN;

    let mut stats = GridStats::default();
    let max_displayable_rows = ((size.height - MARGIN).max(0.0) / cell_h).floor() as usize;
    // Vertical stretch for cell-filling glyphs (box drawing / blocks) so they
    // stay contiguous when CJK metrics make cell_h taller than the font line.
    let stretch_ratio = (cell_h / latin_line_h).max(1.0);

    // Geometry first: it only needs the scrollbar state, so the hit-test rect
    // stays valid even if the render snapshot below fails.
    let scrollbar = match eng.term.scrollbar() {
        Ok(s) => scrollbar_geometry(
            pane_id,
            offset_x,
            offset_y,
            size,
            s.total as usize,
            s.offset as usize,
            s.len as usize,
        ),
        Err(_) => None,
    };

    let snapshot_t = profiling_enabled().then(std::time::Instant::now);
    let Ok(snapshot) = eng.render.update(&eng.term) else {
        return (scrollbar, stats);
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
    // Column of the cursor block, hoisted out of the row loop. The cursor only
    // shows on the active pane, and it lives on the row it is reported for.
    let cursor_cell_x: Option<usize> = if is_active && cursor_visible {
        cursor_vp.map(|c| {
            let cx = c.x as usize;
            if c.at_wide_tail && cx > 0 {
                cx - 1
            } else {
                cx
            }
        })
    } else {
        None
    };
    let cursor_row = if is_active && cursor_visible {
        cursor_vp.map(|c| c.y as usize)
    } else {
        None
    };

    let Ok(mut row_iter) = eng.rows.update(&snapshot) else {
        return (scrollbar, stats);
    };
    if let Some(t) = snapshot_t {
        stats.snapshot_ns = t.elapsed().as_nanos();
    }

    let profiling = profiling_enabled();

    ROW_SCRATCH.with(|slot| {
        let scratch = &mut *slot.borrow_mut();
        // The arena is reused across rows: only the current row's slice has to
        // stay valid, and the cells are re-collected from the top of it each
        // row, so one clear per row is enough.
        scratch.text.clear();

        let mut row_idx: usize = 0;
        while let Some(row) = row_iter.next() {
            if row_idx >= display_lines {
                break;
            }
            let y = offset_y + row_idx as f64 * cell_h;
            let bg_row_y = y;

            let row_sel = row.selection().ok().flatten();
            let row_cursor_x = if cursor_row == Some(row_idx) {
                cursor_cell_x
            } else {
                None
            };

            // Collect this row's cells.
            let scan_t = profiling.then(std::time::Instant::now);
            scratch.cells.clear();
            let row_text_start = scratch.text.len();
            if let Ok(mut cell_iter) = eng.cells.update(row) {
                let mut x: usize = 0;
                while let Some(c) = cell_iter.next() {
                    if x >= cols {
                        break;
                    }
                    // One `raw_cell` fetch feeds the wide flag: every cell
                    // accessor is its own FFI call, and asking twice for the
                    // same cell was pure overhead.
                    let raw = c.raw_cell().ok();
                    let wide_kind = raw
                        .and_then(|r| r.wide().ok())
                        .unwrap_or(CellWide::Narrow);
                    let is_spacer = matches!(wide_kind, CellWide::SpacerTail | CellWide::SpacerHead);
                    let wide = matches!(wide_kind, CellWide::Wide);

                    // `graphemes_utf8` answers an empty string for cells without
                    // text, so the separate `has_text` query — another FFI call
                    // per cell — is not needed to decide whether to ask for the
                    // text at all.
                    let text_start = scratch.text.len();
                    scratch.glyph.clear();
                    let _ = c.graphemes_utf8(&mut scratch.glyph);
                    // Control codepoints never render as glyphs. The common case
                    // is text without any, so only rebuild the cell's text when
                    // the scan finds something to drop.
                    if has_control_char(&scratch.glyph) {
                        scratch.glyph = scratch
                            .glyph
                            .chars()
                            .filter(|ch| !ch.is_control())
                            .collect();
                    }
                    scratch.text.push_str(&scratch.glyph);
                    let text_end = scratch.text.len();

                    let mut underline = false;
                    let mut strikethrough = false;
                    if c.has_styling().unwrap_or(false) {
                        if let Ok(style) = c.style() {
                            underline = style.underline != Underline::None;
                            strikethrough = style.strikethrough;
                        }
                    }

                    // The foreground only shows up in glyphs and decorations,
                    // so a cell with neither never needs to resolve it.
                    let fg = if text_end > text_start || underline || strikethrough {
                        c.fg_color()
                            .ok()
                            .flatten()
                            .map(rgb_to_color)
                            .unwrap_or(default_fg)
                    } else {
                        default_fg
                    };
                    let bg = c.bg_color().ok().flatten().map(rgb_to_color);

                    let selected = row_sel
                        .map(|s| x as u16 >= s.start_x && x as u16 <= s.end_x)
                        .unwrap_or(false);

                    scratch.cells.push(CellDraw {
                        x,
                        text: text_start..text_end,
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
            stats.cells += scratch.cells.len() as u64;
            if let Some(t) = scan_t {
                stats.scan_ns += t.elapsed().as_nanos();
            }
            let RowScratch { cells, text, .. } = &*scratch;

            // Cell backgrounds: cells sharing a colour in a row collapse into
            // one rect, so a full-width colour band costs a single draw op
            // instead of one per cell. Selected cells and wide-char spacers
            // keep their previous behaviour of not painting a background.
            let (row_top, row_bottom) = row_span(y, cell_h);
            let mut run: FillRun = None;
            for cell in cells {
                let color = match cell.bg {
                    Some(color) if !cell.is_spacer && !cell.selected => color,
                    _ => {
                        flush_run(painter, &mut run, row_top, row_bottom);
                        continue;
                    }
                };
                let x0 = offset_x + cell.x as f64 * cell_w;
                let x1 = x0 + if cell.wide { cell_w * 2.0 } else { cell_w };
                match run {
                    Some((start, end, prev)) if prev == color && end == x0 => {
                        run = Some((start, x1, color));
                    }
                    _ => {
                        flush_run(painter, &mut run, row_top, row_bottom);
                        run = Some((x0, x1, color));
                    }
                }
            }
            flush_run(painter, &mut run, row_top, row_bottom);

            // Selection: one merged rect per row-local range to avoid seams.
            if cols > 0 {
                if let Some(sel) = row_sel {
                    let start = (sel.start_x as usize).min(cols - 1);
                    let end = (sel.end_x as usize).min(cols - 1);
                    let bx = offset_x + start as f64 * cell_w;
                    let ex = offset_x + (end + 1) as f64 * cell_w;
                    let sel_rect = Rect::new(bx, row_top, ex, row_bottom);
                    painter.fill(&sel_rect, colors::BLUE).draw();
                }
            }

            // Underline / strikethrough, merged per colour like the backgrounds.
            for underline_pass in [true, false] {
                let top = if underline_pass {
                    bg_row_y + cell_h - 2.0
                } else {
                    bg_row_y + cell_h / 2.0 - 1.0
                };
                let bottom = top + 2.0;
                let mut run: FillRun = None;
                for cell in cells {
                    let decorated = if underline_pass {
                        cell.underline
                    } else {
                        cell.strikethrough
                    };
                    if !decorated || cell.is_spacer {
                        flush_run(painter, &mut run, top, bottom);
                        continue;
                    }
                    let x0 = offset_x + cell.x as f64 * cell_w;
                    let x1 = x0 + if cell.wide { cell_w * 2.0 } else { cell_w };
                    match run {
                        Some((start, end, prev)) if prev == cell.fg && end == x0 => {
                            run = Some((start, x1, cell.fg));
                        }
                        _ => {
                            flush_run(painter, &mut run, top, bottom);
                            run = Some((x0, x1, cell.fg));
                        }
                    }
                }
                flush_run(painter, &mut run, top, bottom);
            }

            // Cursor block (viewport-relative position from the snapshot).
            if let Some(cx) = row_cursor_x {
                if cx < cols {
                    // Cells are collected in column order, so index == x.
                    let cursor_w = cells
                        .get(cx)
                        .map(|c| if c.wide { cell_w * 2.0 } else { cell_w })
                        .unwrap_or(cell_w);
                    let cursor_x = offset_x + cx as f64 * cell_w;
                    let cursor_rect = Rect::new(cursor_x, row_top, cursor_x + cursor_w, row_bottom);
                    painter.fill(&cursor_rect, cursor_brush).draw();
                }
            }

            // Render each character at its fixed cell position.
            // All characters share the same baseline (baseline_ascent from the
            // mixed-script probe) so Latin and CJK glyphs align correctly.
            //
            // Cells that resolve to the same font and colour are batched into a
            // single glyph run; see `GlyphBatch`.
            let glyph_t = profiling.then(std::time::Instant::now);
            let batch_transform = Affine::translate((0.0, y + baseline_ascent));
            let mut batch: Option<GlyphBatch> = None;
            for cell in cells {
                if cell.is_spacer {
                    continue;
                }
                let s = &text[cell.text.start..cell.text.end];
                // A plain space draws nothing, so it can skip the whole
                // layout lookup and text pass.
                if s.is_empty() || s.as_bytes() == b" " {
                    continue;
                }
                stats.glyphs += 1;

                let fg_color = if row_cursor_x == Some(cell.x) || cell.selected {
                    colors::CRUST
                } else {
                    cell.fg
                };
                let x = offset_x + cell.x as f64 * cell_w;

                let stretch = stretch_ratio > 1.005 && is_cell_filler(s);
                // Cached per text: rebuilding a layout per cell is what made
                // scrolling janky, and the cached run always carries brush
                // index 1 so the cell's foreground can be supplied here.
                let entry = glyph_entry(lcx, fcx, s, font_size);

                // Cell fillers can be scaled to the cell height, which is a
                // per-cell transform, so they never join a batch.
                if !stretch
                    && batch_entry(
                        painter,
                        batch_transform,
                        &entry,
                        x as f32,
                        fg_color,
                        &mut batch,
                        &mut stats,
                    )
                {
                    continue;
                }

                flush_glyph_batch(painter, &mut batch, batch_transform);
                let transform =
                    glyph_transform(s, x, y, baseline_ascent, &entry.layout, stretch, stretch_ratio);
                render_text(
                    painter,
                    transform,
                    &entry.layout,
                    &[Brush::Solid(colors::TEXT), Brush::Solid(fg_color)],
                    true,
                );
            }
            flush_glyph_batch(painter, &mut batch, batch_transform);
            if let Some(t) = glyph_t {
                stats.glyph_ns += t.elapsed().as_nanos();
            }

            // Drop this row's text before the next one appends its own.
            scratch.text.truncate(row_text_start);
            row_idx += 1;
        }
    });

    (scrollbar, stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::{Terminal, TerminalConfig};
    use xilem::masonry::imaging::record::Command;

    /// A terminal fed `text`, with no PTY attached: the renderer only needs the
    /// engine, so the cell-scan path can be exercised headlessly.
    fn terminal_with(text: &[u8]) -> Terminal {
        let terminal = Terminal::new(&TerminalConfig::default());
        terminal.engine().term.vt_write(text);
        terminal
    }

    /// Render one frame and report the grid pass's counters along with the
    /// number of draw commands the scene ended up with.
    fn render(terminal: &Terminal) -> (GridStats, usize) {
        let mut fcx = FontContext::new();
        let mut lcx = LayoutContext::new();
        let mut scene = Scene::new();
        let mut painter = Painter::new(&mut scene);

        let mut guard = terminal.engine();
        let (_, stats) = render_grid(
            &mut painter,
            &mut guard,
            1,
            true,
            Size::new(800.0, 600.0),
            0.0,
            0.0,
            14.0,
            8.0,
            16.0,
            16.0,
            12.0,
            &mut fcx,
            &mut lcx,
        );
        drop(guard);

        let draws = scene
            .commands()
            .iter()
            .filter(|cmd| matches!(cmd, Command::Draw(_)))
            .count();
        (stats, draws)
    }

    #[test]
    fn blank_cells_and_spaces_produce_no_glyphs() {
        let terminal = terminal_with(b"hi\r\n\r\n");
        let (stats, _) = render(&terminal);
        assert!(stats.cells > 0, "the grid should still be scanned");
        assert_eq!(stats.glyphs, 2, "only the two letters are drawn");
    }

    #[test]
    fn control_bytes_are_not_drawn() {
        // A BEL is not text: it must not become a glyph run.
        let terminal = terminal_with(b"a\x07\r\n");
        let (stats, _) = render(&terminal);
        assert_eq!(stats.glyphs, 1);
    }

    #[test]
    fn wide_glyphs_and_trailing_blank_area_render() {
        let terminal = terminal_with("你好ab\r\n".as_bytes());
        let (stats, _) = render(&terminal);
        assert_eq!(stats.glyphs, 4, "two wide and two narrow glyphs");
    }

    #[test]
    fn background_bands_merge_into_one_fill() {
        // A whole row of one background colour is 80 cells but a single band,
        // so the frame must not record a fill per cell.
        let terminal = terminal_with(b"\x1b[41m\x1b[K");
        let (_, draws) = render(&terminal);
        assert!(
            draws < 20,
            "expected the coloured row to merge into one fill, got {draws} draws"
        );
    }

    /// Every glyph the scene ends up drawing, in device space: glyph id,
    /// absolute x/y and brush. Runs are flattened so that batching several
    /// cells into one run is indistinguishable from drawing them one by one.
    fn drawn_glyphs(scene: &Scene) -> Vec<(u32, f64, f64, Color)> {
        use xilem::masonry::imaging::record::{Command, Draw};
        let mut out = Vec::new();
        for command in scene.commands() {
            let Command::Draw(id) = command else { continue };
            let Draw::GlyphRun(run) = scene.draw_op(*id) else {
                continue;
            };
            let color = match run.brush {
                Brush::Solid(color) => color,
                _ => continue,
            };
            for glyph in &run.glyphs {
                let p = run.transform * xilem::masonry::kurbo::Point::new(glyph.x as f64, glyph.y as f64);
                out.push((glyph.id, p.x, p.y, color));
            }
        }
        out
    }

    #[test]
    fn batched_glyph_runs_match_per_cell_drawing() {
        // Mixed content: several colours, a wide cell, CJK, spacing and a
        // selection, all of which split batches at different points.
        let terminal = terminal_with(
            b"\x1b[1;32mgreen\x1b[0m plain \x1b[31mred\x1b[0m \
              \xe4\xbd\xa0\xe5\xa5\xbd\x1b[4munder\x1b[0m |\xe2\x94\x82|\r\n\r\nsecond row\r\n",
        );

        let mut fcx = FontContext::new();
        ensure_symbol_font(&mut fcx);
        let mut lcx = LayoutContext::new();
        let size = Size::new(800.0, 600.0);
        const CELL_W: f64 = 8.0;
        const CELL_H: f64 = 16.0;
        const FONT_SIZE: f32 = 14.0;

        let mut batched = Scene::new();
        {
            let mut painter = Painter::new(&mut batched);
            let mut guard = terminal.engine();
            let _ = render_grid(
                &mut painter, &mut guard, 1, true, size, 0.0, 0.0, FONT_SIZE, CELL_W, CELL_H,
                CELL_H, 12.0, &mut fcx, &mut lcx,
            );
        }

        // Reference: the same content drawn one cell at a time, through
        // masonry's `render_text`, at the same cell positions.
        let mut reference = Scene::new();
        {
            let mut painter = Painter::new(&mut reference);
            let mut guard = terminal.engine();
            let crate::terminal::Engine {
                render,
                term,
                rows: row_iter_ref,
                cells: cell_iter_ref,
            } = &mut *guard;
            let snapshot = render.update(term).expect("snapshot");
            let cols = snapshot.cols().unwrap_or(0) as usize;
            let mut rows_iter = row_iter_ref.update(&snapshot).expect("rows");
            let mut row_idx = 0usize;
            while let Some(row) = rows_iter.next() {
                let y = row_idx as f64 * CELL_H;
                let mut x = 0usize;
                if let Ok(mut cells) = cell_iter_ref.update(row) {
                    while let Some(cell) = cells.next() {
                        if x >= cols {
                            break;
                        }
                        let raw = cell.raw_cell().ok();
                        let is_spacer = raw
                            .and_then(|r| r.wide().ok())
                            .is_some_and(|w| {
                                matches!(w, CellWide::SpacerTail | CellWide::SpacerHead)
                            });
                        if raw.map(|r| r.has_text().unwrap_or(false)).unwrap_or(false) && !is_spacer {
                            let mut text = String::new();
                            let _ = cell.graphemes_utf8(&mut text);
                            // Same skips as the batched path: nothing to draw
                            // for an empty cell or a plain space.
                            if !text.is_empty() && text.as_bytes() != b" " {
                                let fg = cell
                                    .fg_color()
                                    .ok()
                                    .flatten()
                                    .map(rgb_to_color)
                                    .unwrap_or(colors::TEXT);
                                let layout = glyph_entry(&mut lcx, &mut fcx, &text, FONT_SIZE);
                                let transform = glyph_transform(
                                    &text,
                                    x as f64 * CELL_W,
                                    y,
                                    12.0,
                                    &layout.layout,
                                    false,
                                    1.0,
                                );
                                render_text(
                                    &mut painter,
                                    transform,
                                    &layout.layout,
                                    &[Brush::Solid(colors::TEXT), Brush::Solid(fg)],
                                    true,
                                );
                            }
                        }
                        x += 1;
                    }
                }
                row_idx += 1;
            }
        }

        let batched_glyphs = drawn_glyphs(&batched);
        let reference_glyphs = drawn_glyphs(&reference);
        assert!(
            !batched_glyphs.is_empty(),
            "the probe screen should draw something"
        );
        assert_eq!(batched_glyphs.len(), reference_glyphs.len());
        for (batched, reference) in batched_glyphs.iter().zip(&reference_glyphs) {
            assert_eq!(batched.0, reference.0, "glyph id");
            assert_eq!(batched.3, reference.3, "brush");
            assert!(
                (batched.1 - reference.1).abs() < 1e-3 && (batched.2 - reference.2).abs() < 1e-3,
                "position: batched ({}, {}) vs per-cell ({}, {})",
                batched.1,
                batched.2,
                reference.1,
                reference.2
            );
        }
    }

    #[test]
    fn activity_bar_icons_stack_under_the_gear() {
        // One slot for the settings gear, then one per workspace, inside the
        // bar's padding: the tooltip relies on this being exact.
        assert_eq!(
            sidebar_icon_center(0),
            SIDEBAR_PADDING + SIDEBAR_ICON * 1.5
        );
        assert_eq!(
            sidebar_icon_center(1) - sidebar_icon_center(0),
            SIDEBAR_ICON
        );
        assert_eq!(
            sidebar_icon_center(3) - sidebar_icon_center(2),
            SIDEBAR_ICON
        );
    }

    #[test]
    fn tooltip_stays_inside_the_canvas() {
        let area = Size::new(400.0, 200.0);
        const BOX_H: f64 = 60.0;

        // The first icon sits near the top: the box would start above the
        // canvas, so it is pushed down to the margin.
        let (x, y) = tooltip_origin(area, 0, BOX_H);
        assert_eq!(x, 6.0);
        assert!(y >= 4.0, "tooltip top must stay inside, got {y}");

        // A far-down icon pushes the box past the bottom edge: clamp it.
        let (_, y) = tooltip_origin(area, 40, BOX_H);
        assert!(
            y + BOX_H <= area.height,
            "tooltip must fit vertically: {y} + {BOX_H} > {}",
            area.height
        );

        // A middle icon: centred on the slot, offset by the tab bar.
        let (_, y) = tooltip_origin(area, 1, BOX_H);
        let icon_center = sidebar_icon_center(1) - TAB_BAR_HEIGHT;
        assert!((y + BOX_H / 2.0 - icon_center).abs() < 1e-9);
    }

    #[test]
    fn long_paths_keep_their_tail() {
        let path = r"C:\Users\someone\projects\amux\src\terminal_render.rs";
        let short = shorten_path(path, 24);
        assert_eq!(short.chars().count(), 24);
        assert!(short.starts_with('…'));
        assert!(
            path.ends_with(&short['…'.len_utf8()..]),
            "{short} should be the tail of {path}"
        );
        // Short paths are untouched.
        assert_eq!(shorten_path(r"C:\tmp", 24), r"C:\tmp");
    }

    #[test]
    fn long_names_keep_their_head() {
        let name = "Windows PowerShell (Administrator)";
        let short = shorten(name, 16);
        assert_eq!(short.chars().count(), 16);
        assert!(short.ends_with('…'));
        assert!(name.starts_with(&short[..short.len() - '…'.len_utf8()]));
    }
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
    let layout = ui_layout(lcx, fcx, text, size, bold);

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
    ui_layout(lcx, fcx, text, size, bold).width() as f64
}

// Cache of overlay label layouts, keyed by text, size and weight.
//
// The settings panel paints a dozen labels per frame and repaints on every
// pointer move, but the strings never change; each cache hit also serves the
// measuring pass that centres a label.
type UiTextCache = HashMap<(Rc<str>, u32, bool), Rc<Layout<BrushIndex>>>;

thread_local! {
    static UI_TEXT: RefCell<UiTextCache> = RefCell::new(HashMap::new());
}

/// Build (or reuse) the layout of one overlay label.
fn ui_layout(
    lcx: &mut LayoutContext<BrushIndex>,
    fcx: &mut FontContext,
    text: &str,
    size: f32,
    bold: bool,
) -> Rc<Layout<BrushIndex>> {
    let key = (Rc::<str>::from(text), size.to_bits(), bold);
    if let Some(layout) = UI_TEXT.with(|cache| cache.borrow().get(&key).map(Rc::clone)) {
        return layout;
    }
    let mut builder = lcx.ranged_builder(fcx, text, 1.0, true);
    builder.push_default(StyleProperty::FontFamily(term_font_family()));
    builder.push_default(StyleProperty::FontSize(size));
    if bold {
        builder.push_default(StyleProperty::FontWeight(FontWeight::BOLD));
    }
    builder.push(StyleProperty::Brush(BrushIndex(1)), 0..text.len());
    let mut layout = builder.build(text);
    layout.break_all_lines(None);
    let layout = Rc::new(layout);
    UI_TEXT.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= UI_TEXT_LIMIT {
            cache.clear();
        }
        cache.insert(key, Rc::clone(&layout));
    });
    layout
}

/// Cache bound for overlay labels: the panel has a fixed, small string set.
const UI_TEXT_LIMIT: usize = 64;

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

// ─── Activity bar: geometry, hover tooltip, close confirmation ───

/// Width of the left activity bar. The terminal canvas starts right after it,
/// so this is also the canvas' x origin in window coordinates.
pub const SIDEBAR_WIDTH: f64 = 40.0;
/// Padding between the activity bar's edge and its icons.
pub const SIDEBAR_PADDING: f64 = 2.0;
/// Edge length of one activity-bar slot, inner padding included.
pub const SIDEBAR_ICON: f64 = 32.0;
/// Height of the tab bar above the terminal canvas.
///
/// Fixed so the activity-bar tooltip can be placed at the hovered icon's
/// height: the tooltip is painted by the canvas, whose origin sits this far
/// below the window top.
pub const TAB_BAR_HEIGHT: f64 = 28.0;

/// Vertical centre of activity-bar icon `index`, in window coordinates.
///
/// The bar stacks one 32px slot per workspace under the settings gear, so the
/// offset is fully determined by the index (and the fixed gaps above).
pub fn sidebar_icon_center(index: usize) -> f64 {
    SIDEBAR_PADDING + SIDEBAR_ICON + index as f64 * SIDEBAR_ICON + SIDEBAR_ICON / 2.0
}

/// Top-left corner of the activity-bar tooltip, in canvas coordinates.
///
/// It sits just right of the bar, centred on the hovered icon's height, and is
/// clamped so it never leaves the canvas.
fn tooltip_origin(area: Size, icon_index: usize, box_h: f64) -> (f64, f64) {
    // The canvas starts at the bar's right edge, so a small inset from x = 0
    // is a small gap from the bar.
    const X: f64 = 6.0;
    let center = sidebar_icon_center(icon_index) - TAB_BAR_HEIGHT;
    let max_y = (area.height - box_h - 4.0).max(4.0);
    (X, (center - box_h / 2.0).clamp(4.0, max_y))
}

/// Buttons of the close-workspace confirmation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmAction {
    Confirm,
    Cancel,
}

/// Truncate `text` to `max` characters, marking the cut with an ellipsis.
fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept)
}

/// Shorten a filesystem path from the left: the tail — the directory the shell
/// is actually in — is the informative part.
fn shorten_path(path: &str, max: usize) -> String {
    let count = path.chars().count();
    if count <= max {
        return path.to_string();
    }
    let skip = count - max.saturating_sub(1);
    let tail: String = path.chars().skip(skip).collect();
    format!("…{}", tail)
}

/// Draw the activity-bar hover tooltip: workspace name, terminal title and the
/// shell's live directory.
///
/// The tooltip belongs to a 40px-wide bar, which cannot hold it, so it is
/// painted by the terminal canvas just right of the bar, centred on the
/// hovered icon's height.
pub fn render_sidebar_tooltip(
    painter: &mut Painter<'_, Scene>,
    app: &App,
    area: Size,
    fcx: &mut FontContext,
    lcx: &mut LayoutContext<BrushIndex>,
) {
    let Some(tip) = &app.hover_tooltip else {
        return;
    };
    // The modal overlays own the screen while they are open.
    if app.show_settings || app.pending_group_close.is_some() {
        return;
    }

    const PAD_X: f64 = 10.0;
    const PAD_Y: f64 = 7.0;
    const LINE_H: f64 = 16.0;
    const NAME_SIZE: f32 = 12.5;
    const BODY_SIZE: f32 = 11.0;
    const MAX_TEXT: usize = 56;

    let name = shorten(&tip.name, MAX_TEXT);
    let title = tip
        .title
        .as_deref()
        .filter(|title| !title.is_empty() && *title != tip.name.as_str())
        .map(|title| shorten(title, MAX_TEXT));
    let cwd = tip
        .cwd
        .as_deref()
        .map(|path| shorten_path(path, MAX_TEXT))
        .unwrap_or_else(|| "(未知路径)".to_string());

    let mut width = measure_text(fcx, lcx, &name, NAME_SIZE, true);
    width = width.max(measure_text(fcx, lcx, &cwd, BODY_SIZE, false));
    if let Some(title) = &title {
        width = width.max(measure_text(fcx, lcx, title, BODY_SIZE, false));
    }

    let lines = 2 + usize::from(title.is_some());
    // `area` is the canvas, which already starts right of the activity bar.
    let box_w = (width + PAD_X * 2.0).clamp(150.0, (area.width - 16.0).max(150.0));
    let box_h = PAD_Y * 2.0 + lines as f64 * LINE_H;

    // Canvas-local: just right of the activity bar, centred on the icon.
    let (x, y) = tooltip_origin(area, tip.group_index, box_h);

    let panel = RoundedRect::new(x, y, x + box_w, y + box_h, 6.0);
    painter.fill(&panel, colors::SURFACE0).draw();
    painter.stroke(&panel, &Stroke::new(1.0), colors::SURFACE1).draw();

    let mut baseline = y + PAD_Y + 11.0;
    draw_ui_text(painter, fcx, lcx, &name, x + PAD_X, baseline, NAME_SIZE, colors::TEXT, true);
    baseline += LINE_H;
    if let Some(title) = &title {
        draw_ui_text(painter, fcx, lcx, title, x + PAD_X, baseline, BODY_SIZE, colors::SUBTEXT, false);
        baseline += LINE_H;
    }
    draw_ui_text(painter, fcx, lcx, &cwd, x + PAD_X, baseline, BODY_SIZE, colors::BLUE, false);
}

/// Draw the close-workspace confirmation over the terminal area.
///
/// Interactive rects are collected into `app.confirm_items` and are hit-tested
/// by `App::handle_mouse_down` on the next click.
pub fn render_confirm_overlay(
    painter: &mut Painter<'_, Scene>,
    app: &mut App,
    area: Size,
    fcx: &mut FontContext,
    lcx: &mut LayoutContext<BrushIndex>,
) {
    const PANEL_W: f64 = 360.0;
    const PAD: f64 = 18.0;
    const TITLE_H: f64 = 34.0;
    const BODY_H: f64 = 44.0;
    const FOOTER_H: f64 = 34.0;
    const CHIP_H: f64 = 24.0;
    const CHIP_W: f64 = 64.0;

    app.confirm_items.clear();
    let Some(index) = app.pending_group_close else {
        return;
    };
    let Some(group) = app.groups.get(index) else {
        return;
    };
    let name = group.name.clone();
    let tab_count = group.tabs.len();
    let pane_count: usize = group.tabs.iter().map(|tab| tab.pane_tree.pane_count()).sum();

    let hover = app.hover_pos;

    let panel_h = PAD + TITLE_H + BODY_H + FOOTER_H + PAD;
    // Shrink on narrow canvases so the dialog always fits horizontally.
    let panel_w = PANEL_W.min((area.width - 16.0).max(200.0));
    let px = ((area.width - panel_w) / 2.0).max(8.0);
    let py = ((area.height - panel_h) / 2.0).max(8.0);

    // Scrim: dim the terminal behind the modal.
    painter
        .fill(&Rect::new(0.0, 0.0, area.width, area.height), Color::from_rgba8(0x11, 0x11, 0x1b, 0xb4))
        .draw();

    let panel = RoundedRect::new(px, py, px + panel_w, py + panel_h, 10.0);
    painter.fill(&panel, colors::BASE).draw();
    painter.stroke(&panel, &Stroke::new(1.0), colors::SURFACE1).draw();

    let content_left = px + PAD;
    let content_right = px + panel_w - PAD;

    // Header: title plus a close chip on the right (cancels).
    draw_ui_text(painter, fcx, lcx, "关闭工作区", content_left, py + PAD + 17.0, 15.0, colors::TEXT, true);
    let title_w = measure_text(fcx, lcx, "关闭工作区", 15.0, true);
    draw_ui_text(
        painter,
        fcx,
        lcx,
        "CLOSE WORKSPACE",
        content_left + title_w + 10.0,
        py + PAD + 17.0,
        10.0,
        colors::SUBTEXT,
        false,
    );

    let close_rect = HitRect::new(content_right - 28.0, py + PAD, 28.0, 22.0);
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
        py + PAD + 15.5,
        13.0,
        colors::TEXT,
        false,
    );
    app.confirm_items.push((ConfirmAction::Cancel, inflate(&close_rect, 2.0)));

    // Body: what would be lost.
    let body_y = py + PAD + TITLE_H;
    let shown_name = shorten(&name, 32);
    draw_ui_text(
        painter,
        fcx,
        lcx,
        &format!("确定要关闭 “{}” 吗？", shown_name),
        content_left,
        body_y + 15.0,
        13.0,
        colors::TEXT,
        false,
    );
    draw_ui_text(
        painter,
        fcx,
        lcx,
        &format!("{} 个标签页、{} 个终端将被结束。", tab_count, pane_count),
        content_left,
        body_y + 33.0,
        11.0,
        colors::SUBTEXT,
        false,
    );

    // Footer: cancel on the left of the destructive action.
    let chip_y = py + PAD + TITLE_H + BODY_H + (FOOTER_H - CHIP_H) / 2.0;
    let confirm_rect = HitRect::new(content_right - CHIP_W, chip_y, CHIP_W, CHIP_H);
    let cancel_rect = HitRect::new(content_right - CHIP_W * 2.0 - 8.0, chip_y, CHIP_W, CHIP_H);

    let cancel_hover = hover.map_or(false, |(hx, hy)| {
        hx >= cancel_rect.x && hx <= cancel_rect.x + cancel_rect.width
            && hy >= cancel_rect.y && hy <= cancel_rect.y + cancel_rect.height
    });
    draw_chip(painter, &cancel_rect, cancel_hover);
    draw_ui_text_centered(
        painter,
        fcx,
        lcx,
        "取消",
        cancel_rect.x + cancel_rect.width / 2.0,
        chip_y + CHIP_H / 2.0 + 4.5,
        12.0,
        colors::TEXT,
        false,
    );
    app.confirm_items.push((ConfirmAction::Cancel, inflate(&cancel_rect, 2.0)));

    let confirm_hover = hover.map_or(false, |(hx, hy)| {
        hx >= confirm_rect.x && hx <= confirm_rect.x + confirm_rect.width
            && hy >= confirm_rect.y && hy <= confirm_rect.y + confirm_rect.height
    });
    let chip = RoundedRect::new(
        confirm_rect.x,
        confirm_rect.y,
        confirm_rect.x + confirm_rect.width,
        confirm_rect.y + confirm_rect.height,
        6.0,
    );
    let fill = if confirm_hover {
        Color::from_rgba8(0xe6, 0x4d, 0x4d, 0xff)
    } else {
        Color::from_rgba8(0xc9, 0x3d, 0x3d, 0xff)
    };
    painter.fill(&chip, fill).draw();
    draw_ui_text_centered(
        painter,
        fcx,
        lcx,
        "关闭",
        confirm_rect.x + confirm_rect.width / 2.0,
        chip_y + CHIP_H / 2.0 + 4.5,
        12.0,
        Color::WHITE,
        true,
    );
    app.confirm_items.push((ConfirmAction::Confirm, inflate(&confirm_rect, 2.0)));
}
















