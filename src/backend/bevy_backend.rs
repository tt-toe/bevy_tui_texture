use std::sync::Arc;

use crate::backend::CellInstance;
use crate::backend::Viewport;
use crate::backend::rasterize::rasterize_glyph;
use crate::colors::Rgb;
use crate::fonts::Fonts;
use crate::utils::text_atlas::Key;
use ratatui::buffer::Cell;
use ratatui::text::Line;
use rustybuzz::UnicodeBuffer;

const NULL_CELL: Cell = Cell::new("");

/// Cached geometry for one row, from the last flush that (re)generated it.
/// Reused verbatim by a later flush when the row is clean (`dirty_rows[y]
/// == false`) and the atlas hasn't reassigned any slot since - see
/// `BevyTerminalBackend::flush`. Instances carry absolute pixel positions
/// (the row's `y` included), so a cached row is appended to a frame's
/// payload verbatim, with no rebasing.
#[derive(Default, Clone)]
struct RowGeometry {
    /// One instance per covered cell - see [`CellInstance`].
    cells: Vec<CellInstance>,
    /// Extra glyphs landing in a cell `cells` already covers (combining
    /// marks shaped as separate glyphs). Empty for almost every row.
    overlays: Vec<CellInstance>,
    /// `Atlas::generation()` as of when this geometry was (re)computed.
    /// A mismatch against the atlas's current generation means some slot
    /// referenced by these instances may have been reassigned to a
    /// different glyph since - the row must be regenerated, not reused.
    atlas_generation: u64,
}

/// One glyph of the row being shaped: a [`ShapedGlyph`] rebased onto the
/// row (its cluster is a byte offset into the whole row string) and tagged
/// with the [`FontRun`] it came from.
///
/// [`ShapedGlyph`]: crate::utils::shape_cache::ShapedGlyph
#[derive(Clone, Copy)]
struct RowGlyph {
    glyph_id: u32,
    cluster: u32,
    x_advance: i32,
    x_offset: i32,
    run: u16,
}

/// A maximal stretch of a row whose cells all resolve to the same font
/// (and fake styling), shaped as one rustybuzz run. `bytes` indexes the
/// row string `shape_row` builds by concatenating the cells' symbols.
struct FontRun<'f> {
    font: &'f crate::fonts::Font,
    fake_bold: bool,
    fake_italic: bool,
    bytes: std::ops::Range<usize>,
}

/// Splits a row into [`FontRun`]s via [`Fonts::font_for_cell`]. Shaping a
/// whole row with a single font would render every char that font lacks
/// as a missing glyph even when a fallback font has it. A cell with an
/// empty symbol (the hidden half of a wide char) contributes no text, so
/// it just extends whichever run it follows.
fn font_runs<'f>(
    fonts: &'f Fonts,
    cells: &[Cell],
    choices: &mut crate::utils::shape_cache::FontChoiceCache,
) -> Vec<FontRun<'f>> {
    let mut runs: Vec<FontRun<'f>> = Vec::new();
    let mut end = 0;
    for cell in cells {
        let start = end;
        end += cell.symbol().len();
        if start == end {
            if let Some(run) = runs.last_mut() {
                run.bytes.end = end;
            }
            continue;
        }
        let (font, fake_bold, fake_italic) = fonts.font_for_cell_cached(cell, choices);
        // Fake styling only applies with real per-style font slots.
        let styled = cfg!(feature = "bold_italic_fonts");
        let (fake_bold, fake_italic) = (styled && fake_bold, styled && fake_italic);
        match runs.last_mut() {
            Some(run)
                if run.font.id() == font.id()
                    && run.fake_bold == fake_bold
                    && run.fake_italic == fake_italic =>
            {
                run.bytes.end = end;
            }
            _ => runs.push(FontRun {
                font,
                fake_bold,
                fake_italic,
                bytes: start..end,
            }),
        }
    }
    runs
}

#[cfg(test)]
mod font_run_tests {
    use super::*;
    use crate::fonts::Font;

    const MPLUS: &[u8] = include_bytes!("../../examples/assets/fonts/Mplus1Code-Regular.ttf");
    const FUSION: &[u8] =
        include_bytes!("../../examples/assets/fonts/fusion-pixel-10px-monospaced-ja.ttf");

    fn cells(symbols: &[&str]) -> Vec<Cell> {
        symbols
            .iter()
            .map(|symbol| {
                let mut cell = Cell::default();
                cell.set_symbol(symbol);
                cell
            })
            .collect()
    }

    /// A row is not shaped with its first cell's font alone: a char that
    /// font lacks gets a run in a fallback font, at any column.
    #[test]
    fn chars_missing_from_the_first_font_get_a_fallback_run() {
        // M+ (tried first) has no Hangul; fusion-pixel (last resort) does.
        let mplus = Font::new(MPLUS).expect("M+ parses");
        let fusion = Font::new(FUSION).expect("fusion-pixel parses");
        let (mplus_id, fusion_id) = (mplus.id(), fusion.id());
        let mut fonts = Fonts::new(fusion, 16);
        fonts.add_regular_fonts([mplus]);

        let row = cells(&["a", "b", "한", "", "c", "d"]);
        let mut choices = crate::utils::shape_cache::FontChoiceCache::new();
        let runs: Vec<_> = font_runs(&fonts, &row, &mut choices)
            .iter()
            .map(|run| (run.font.id(), run.bytes.clone()))
            .collect();
        // a b | 한 (3 bytes) + its empty spacer | c d
        assert_eq!(
            runs,
            vec![(mplus_id, 0..2), (fusion_id, 2..5), (mplus_id, 5..7)]
        );
    }

    #[test]
    fn a_single_font_row_is_one_run() {
        let fonts = Fonts::new(Font::new(MPLUS).expect("M+ parses"), 16);
        let mut choices = crate::utils::shape_cache::FontChoiceCache::new();
        let runs = font_runs(&fonts, &cells(&["h", "i", " ", "あ", ""]), &mut choices);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].bytes, 0..6);
    }
}

/// Horizontal squeeze for a glyph whose advance (`advance_px`) is wider
/// than the cells it occupies (`cells_px`). The cell width comes from the
/// narrowest font, so a wider fallback font's glyphs would otherwise have
/// their right side clipped. Overflow of a pixel or less is left alone:
/// a font whose advance merely rounds down to the cell width (typically
/// the primary one) renders exactly as it always has.
fn fit_to_cells(advance_px: f32, cells_px: f32) -> f32 {
    if advance_px > cells_px + 1.0 {
        cells_px / advance_px
    } else {
        1.0
    }
}

#[cfg(test)]
mod fit_to_cells_tests {
    use super::fit_to_cells;

    #[test]
    fn a_rounded_down_advance_is_not_squeezed() {
        assert_eq!(fit_to_cells(6.4, 6.0), 1.0);
        assert_eq!(fit_to_cells(12.8, 12.0), 1.0);
        assert_eq!(fit_to_cells(0.0, 6.0), 1.0); // synthesized fast-path glyphs
    }

    #[test]
    fn a_wider_fallback_glyph_is_squeezed_to_its_cells() {
        assert_eq!(fit_to_cells(8.0, 6.0), 0.75);
        assert_eq!(fit_to_cells(16.0, 12.0), 0.75); // wide char, two cells
    }
}

/// A ratatui backend optimized for Bevy integration.
///
/// - No lifetime parameters
/// - Fonts are shared via Arc
/// - Pure CPU state: cell grid, dirty tracking, text shaping/rasterization,
///   and vertex generation. No `Device`/`Queue`, no GPU resources at all -
///   those live in the render-world `TerminalGpuState` (`backend/mod.rs`),
///   built from the `TerminalDrawPayload` this backend hands off each dirty
///   frame (see `take_draw_payload`)
/// - No Surface management, renders directly to textures
/// - Simplified for Bevy's rendering pipeline
pub struct BevyTerminalBackend {
    // ====== Terminal dimensions ======
    pub(super) cols: u16,
    pub(super) rows: u16,

    // ====== Terminal state ======
    pub(super) cells: Vec<Cell>,
    pub(super) dirty_rows: Vec<bool>,
    /// Set by `draw()` when ratatui's internal buffer diff contained at
    /// least one changed cell; cleared at the start of every `draw()` call.
    /// Lets `Tui::draw` skip marking itself dirty (and thus skip the GPU
    /// render + copy) when a redraw produces byte-identical content.
    pub(super) cells_changed_last_draw: bool,
    pub(super) cursor: (u16, u16),
    pub(super) viewport: Viewport,
    /// Rows needing re-render, accumulated across flushes since the last
    /// `take_draw_payload`: each `flush()` unions ratatui's dirty rows in;
    /// `take_draw_payload` consumes and clears it. Rows reshaped in
    /// `flush()`'s pass 2 due to atlas eviction are NOT included - eviction
    /// only invalidates a row's cached UVs for a future redraw of that row;
    /// the pixels already rendered to the destination texture from its last
    /// actual redraw remain correct, so no GPU re-render is owed for it
    /// this frame.
    pub(super) rows_dirty_since_take: Vec<bool>,
    /// True whenever the destination texture's current content can't be
    /// trusted to already hold everything up to the last taken payload
    /// (just created / resized / cleared / fonts swapped, or a main-world
    /// pending-payload overwrite - see `Tui::flush` and
    /// `Tui::apply_pending_resize`). While set, `take_draw_payload` always
    /// produces a full payload (`TerminalDrawPayload::load_previous ==
    /// false`) instead of one covering only dirty rows.
    pub(super) full_redraw_needed: bool,

    // ====== Font management (Arc, no lifetime) ======
    // The atlas, plan cache and pending glyph uploads are not here: they
    // live in `fonts.shared_cpu_state` (reachable via
    // `Fonts::with_shared_cpu_state`), shared by every terminal that holds
    // the same `Arc<Fonts>`, so a glyph is rasterized and queued for GPU
    // upload once per font, not once per terminal.
    pub(super) fonts: Arc<Fonts>,
    pub(super) buffer: UnicodeBuffer,
    pub(super) row: String,
    pub(super) rowmap: Vec<u16>,
    /// Scratch for `shape_row`: which cells of the row being shaped already
    /// have a cell instance (so a further glyph there becomes an overlay).
    covered: Vec<bool>,
    /// Scratch for `shape_row`: the row's glyphs, in shaping order.
    glyphs: Vec<RowGlyph>,
    /// cmap lookup cache for the ASCII fast path (feature
    /// `ascii_fast_shaping`) - `None` means "checked, this font
    /// has no glyph for this char", cached to avoid re-probing every
    /// frame. Keyed by `(font id, char)` since a terminal's font can
    /// change via `update_fonts` (which clears this) and, in principle,
    /// differ row to row if multiple fonts were ever in play.
    #[cfg(feature = "ascii_fast_shaping")]
    pub(super) ascii_glyph_cache:
        std::collections::HashMap<(u64, char), Option<rustybuzz::ttf_parser::GlyphId>>,

    // ====== Draw data (owned) ======
    // `flush()` only maintains `row_geometry`; the full-frame or
    // dirty-rows-only instance list is assembled on demand in
    // `take_draw_payload`.
    /// Per-row geometry cache, indexed by row `y`, letting `flush()` skip
    /// reshaping+re-rasterizing rows `dirty_rows` says are unchanged. See
    /// `RowGeometry`.
    row_geometry: Vec<RowGeometry>,

    // ====== Color settings ======
    pub(super) reset_fg: Rgb,
    pub(super) reset_bg: Rgb,
    /// If true, a cell whose *effective* background is `Color::Reset`
    /// (accounting for `Modifier::REVERSED` - see the color-selection logic
    /// in `shape_row`) gets background alpha 0 instead of 255, making it
    /// show through to whatever is behind the terminal's surface (a
    /// world-quad with `AlphaMode::Blend`, or a transparent-background UI
    /// node). Localized entirely to `CellInstance::bg` - the cell pipeline
    /// writes that RGBA as-is wherever the glyph has no coverage.
    pub(super) transparent_reset_bg: bool,
    /// Color shown before any content has been drawn (or while nothing is
    /// drawn at all - e.g. mid-resize): the render world's `LoadOp::Clear`
    /// color whenever a draw payload's vertex data is empty. Carried on
    /// every `TerminalDrawPayload` as `clear_color` (see `take_draw_payload`).
    pub(super) initial_fill: [u8; 4],
}

/// Builder for BevyTerminalBackend. Fully synchronous, pure CPU.
pub struct TerminalBuilder {
    fonts: Arc<Fonts>,
    cols: u16,
    rows: u16,
    reset_fg: Rgb,
    reset_bg: Rgb,
    viewport: Viewport,
    transparent_reset_bg: bool,
    initial_fill: [u8; 4],
}

impl TerminalBuilder {
    /// Create a new builder with the given fonts.
    pub fn new(fonts: Arc<Fonts>) -> Self {
        Self {
            fonts,
            cols: 80,
            rows: 24,
            reset_fg: [255, 255, 255], // WHITE
            reset_bg: [0, 0, 0],       // BLACK
            viewport: Viewport::Full,
            transparent_reset_bg: false,
            initial_fill: [0, 0, 0, 255],
        }
    }

    /// Set terminal dimensions (columns, rows).
    pub fn with_dimensions(mut self, cols: u16, rows: u16) -> Self {
        self.cols = cols;
        self.rows = rows;
        self
    }

    /// Set default foreground color.
    pub fn with_reset_fg(mut self, color: Rgb) -> Self {
        self.reset_fg = color;
        self
    }

    /// Set default background color.
    pub fn with_reset_bg(mut self, color: Rgb) -> Self {
        self.reset_bg = color;
        self
    }

    /// Set viewport mode.
    pub fn with_viewport(mut self, viewport: Viewport) -> Self {
        self.viewport = viewport;
        self
    }

    /// If `true`, cells whose effective background is `Color::Reset` render
    /// with alpha 0 instead of opaque `reset_bg` - see the field doc on
    /// `BevyTerminalBackend::transparent_reset_bg`. Default `false`.
    pub fn with_transparent_reset_bg(mut self, transparent: bool) -> Self {
        self.transparent_reset_bg = transparent;
        self
    }

    /// Color shown before any content has been drawn. Default opaque black
    /// (`[0, 0, 0, 255]`) - see the field doc on
    /// `BevyTerminalBackend::initial_fill`.
    pub fn with_initial_fill(mut self, initial_fill: [u8; 4]) -> Self {
        self.initial_fill = initial_fill;
        self
    }

    /// Build the BevyTerminalBackend.
    ///
    /// This is synchronous (unlike the original async Builder).
    /// Pure CPU construction - no `Device`/`Queue` needed at all. The
    /// corresponding GPU resources (glyph atlas texture, compositor
    /// pipelines) are created lazily in the render world on this font's
    /// first extract (see `SharedFontGpuState` in `backend/mod.rs` and the
    /// render-world store in `bevy_plugin.rs`) - and the CPU-side atlas
    /// index/plan cache lazily on this `Fonts`' first use (see
    /// `Fonts::with_shared_cpu_state`), not here.
    pub fn build(self) -> BevyTerminalBackend {
        BevyTerminalBackend {
            cols: self.cols,
            rows: self.rows,
            cells: vec![],
            dirty_rows: vec![],
            cells_changed_last_draw: false,
            cursor: (0, 0),
            viewport: self.viewport,
            rows_dirty_since_take: vec![],
            full_redraw_needed: true,
            fonts: self.fonts,
            buffer: UnicodeBuffer::new(),
            row: String::new(),
            rowmap: vec![],
            covered: vec![],
            glyphs: vec![],
            #[cfg(feature = "ascii_fast_shaping")]
            ascii_glyph_cache: std::collections::HashMap::new(),
            row_geometry: vec![],
            reset_fg: self.reset_fg,
            reset_bg: self.reset_bg,
            transparent_reset_bg: self.transparent_reset_bg,
            initial_fill: self.initial_fill,
        }
    }
}

/// A tiny-skia pixmap's alpha channel - the coverage mask the R8 glyph
/// atlas stores (tiny-skia pixels are RGBA byte order).
fn pixmap_coverage(pixmap: &tiny_skia::Pixmap) -> Vec<u8> {
    pixmap
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|rgba| rgba[3])
        .collect()
}

/// Coverage mask of a rasterized glyph: `rasterize_glyph` yields RGBA8
/// packed little-endian into `u32`s, so alpha is the top byte.
fn glyph_coverage(image: &[u32]) -> Vec<u8> {
    image.iter().map(|rgba| (rgba >> 24) as u8).collect()
}

/// Pixel coordinate as the `u16` a [`CellInstance`] stores. Terminal
/// textures are bounded by the GPU's max texture dimension, far below
/// `u16::MAX`; saturating keeps an absurd grid from wrapping around.
fn px_u16(px: u32) -> u16 {
    px.min(u16::MAX as u32) as u16
}

impl BevyTerminalBackend {
    /// Whether the most recent `draw()` call's ratatui buffer diff touched
    /// at least one cell. `false` means the redraw was byte-identical to
    /// the previous frame - callers use this to skip GPU work entirely for
    /// unchanged content.
    pub(crate) fn cells_changed_last_draw(&self) -> bool {
        self.cells_changed_last_draw
    }

    /// Update the grid dimensions used by `Backend::size()`/`window_size()`.
    /// Pure bookkeeping - the actual cell/dirty-tracking buffers (`cells`,
    /// `dirty_rows`) are resized lazily inside `draw()` based on the bounds
    /// this affects, the same path a fresh terminal's first draw already
    /// goes through. Callers
    /// must also resize ratatui's own front/back buffers (`Terminal::resize`
    /// or the diff-driving `autoresize` on the next `Terminal::draw`) - this
    /// method only updates what `BevyTerminalBackend` owns directly.
    pub(crate) fn resize(&mut self, cols: u16, rows: u16) {
        self.cols = cols;
        self.rows = rows;

        // A row's cached geometry was computed for the previous grid
        // width - row `y` may map entirely different content at the new
        // one (even if the corresponding ratatui cells happen to diff as
        // "unchanged" in isolation). Clearing (rather than resizing)
        // `dirty_rows` here means the next `draw()`'s
        // `resize(new_height, true)` treats every row as newly added,
        // marking all of them dirty.
        self.dirty_rows.clear();
        self.row_geometry.clear();

        // The destination texture is about to be recreated at the new
        // size (see `Tui::apply_pending_resize`) - nothing rendered to it
        // so far is meaningful at the new dimensions, so the next payload
        // must be a full one, not a dirty-rows-only partial.
        self.rows_dirty_since_take.clear();
        self.full_redraw_needed = true;
    }

    /// Pre-populate programmatic glyphs into the texture atlas.
    ///
    /// This method rasterizes all special glyphs (box-drawing, block elements, braille, powerline)
    /// using tiny-skia and queues them in `pending_cache_updates` - pure CPU work, no GPU upload
    /// here (there is no GPU resource to upload to yet; the render world does that on this
    /// terminal's first extract, exactly like any other glyph). Glyphs not yet implemented are
    /// silently skipped (not an error).
    pub fn populate_programmatic_glyphs(&mut self) {
        use crate::backend::programmatic_glyphs::{
            all_programmatic_glyphs, render_programmatic_glyph,
        };
        use crate::utils::text_atlas::Key;
        use ratatui::style::Modifier;

        let width = self.fonts.min_width_px();
        let height = self.fonts.height_px();
        let font_id = self.fonts.last_resort_id();

        tracing::debug!(
            "Pre-populating {} programmatic glyphs ({}x{} px)...",
            all_programmatic_glyphs().count(),
            width,
            height
        );

        let mut populated_count = 0;
        let mut skipped_count = 0;
        for unicode_char in all_programmatic_glyphs() {
            // Render glyph to bitmap using tiny-skia
            let pixmap = match render_programmatic_glyph(unicode_char, width, height) {
                Some(p) => p,
                None => {
                    // Skip glyphs not yet implemented
                    skipped_count += 1;
                    continue;
                }
            };

            let bitmap = pixmap_coverage(&pixmap);

            // Create atlas key
            let key = Key {
                style: Modifier::empty(),
                glyph: unicode_char as u32,
                font: font_id,
            };

            // Get atlas slot (this allocates space in the shared atlas -
            // shared with every other terminal using this same `Fonts`)
            // and queue the bitmap for GPU upload.
            self.fonts.with_shared_cpu_state(|shared| {
                let rect = shared.cached.get(&key, width, height);
                shared.pending_cache_updates.push((*rect, bitmap));
            });

            populated_count += 1;
        }

        tracing::debug!(
            "Successfully pre-populated {} programmatic glyphs ({} skipped - not yet implemented)",
            populated_count,
            skipped_count
        );
    }

    /// Build the CPU-computed draw payload for the render world to consume:
    /// the cell instances for whichever rows need re-rendering, plus this
    /// backend's font identity (`font_key`, see [`Fonts::identity`]) so the
    /// render world knows which shared atlas/pipelines to render against.
    /// Called only when the terminal is dirty (see `Tui::flush`).
    ///
    /// Concatenates `row_geometry` (maintained by `flush()`), choosing
    /// between two shapes:
    /// - **Full** (`load_previous == false`): every row, drawn after a
    ///   `LoadOp::Clear` to `initial_fill`. Chosen whenever
    ///   `full_redraw_needed` is set (destination content can't be trusted)
    ///   or every row happens to be dirty anyway.
    /// - **Partial** (`load_previous == true`): only rows in
    ///   `rows_dirty_since_take`, each preceded by a row-clear instance (a
    ///   full-row rectangle in `initial_fill`, glyph coverage off). The
    ///   pass uses `LoadOp::Load`, preserving every other row; the clear
    ///   instance resets whatever part of the row no cell instance covers
    ///   (cells with nothing to shape emit no instance).
    ///
    /// Glyph rasterizations are NOT part of this payload - see
    /// [`Self::take_shared_glyph_uploads`].
    pub(crate) fn take_draw_payload(&mut self) -> crate::backend::TerminalDrawPayload {
        use ratatui::backend::Backend;

        let bounds = Backend::size(self).unwrap_or(ratatui::layout::Size {
            width: 0,
            height: 0,
        });
        let height = bounds.height as usize;

        // All rows dirty is cheaper handled as full (no row-clear
        // instances). A `rows_dirty_since_take` shorter than `height`
        // (take called before any draw at this size) never counts as "all
        // dirty" - `full_redraw_needed` (set on every path that grows the
        // grid) is what guarantees correctness for that case instead.
        let all_dirty = height > 0
            && self.rows_dirty_since_take.len() >= height
            && self.rows_dirty_since_take[..height].iter().all(|&d| d);
        let full = self.full_redraw_needed || all_dirty;

        let cell_width_px = self.fonts.min_width_px();
        let cell_height_px = self.fonts.height_px();
        let row_width_px = px_u16(bounds.width as u32 * cell_width_px);

        // `row_geometry` may not reach `y` yet if this is called before the
        // first draw at the current size - such rows are skipped;
        // `full_redraw_needed` guarantees that only happens while `full`,
        // where the `LoadOp::Clear` already covers them.
        let redraw = |y: usize| full || self.rows_dirty_since_take.get(y).copied().unwrap_or(false);
        let rows = || {
            (0..height)
                .filter(|&y| redraw(y))
                .filter_map(|y| self.row_geometry.get(y).map(|row| (y, row)))
        };

        // Sized exactly up front: one allocation, no regrowth while rows
        // are appended.
        let clears = if full {
            0
        } else {
            (0..height).filter(|&y| redraw(y)).count()
        };
        let cell_count = clears + rows().map(|(_, row)| row.cells.len()).sum::<usize>();
        let overlay_count = rows().map(|(_, row)| row.overlays.len()).sum::<usize>();
        let mut instances = Vec::with_capacity(cell_count + overlay_count);

        for y in (0..height).filter(|&y| redraw(y)) {
            if !full {
                instances.push(CellInstance {
                    rect: [
                        0,
                        px_u16(y as u32 * cell_height_px),
                        row_width_px,
                        px_u16(cell_height_px),
                    ],
                    uv: [0, 0],
                    fg: [0; 4],
                    bg: self.initial_fill,
                });
            }
            if let Some(row) = self.row_geometry.get(y) {
                instances.extend_from_slice(&row.cells);
            }
        }
        debug_assert_eq!(instances.len(), cell_count);
        if overlay_count > 0 {
            for (_, row) in rows() {
                instances.extend_from_slice(&row.overlays);
            }
        }

        self.rows_dirty_since_take
            .iter_mut()
            .for_each(|d| *d = false);
        self.full_redraw_needed = false;

        crate::backend::TerminalDrawPayload {
            screen_width_px: (bounds.width as u32 * cell_width_px) as f32,
            screen_height_px: (bounds.height as u32 * cell_height_px) as f32,
            clear_color: self.initial_fill,
            font_key: self.fonts.identity(),
            load_previous: !full,
            instances,
            cell_count: cell_count as u32,
        }
    }

    /// Regenerates `row_geometry[y]` in place via `shape_row`, stamping it
    /// with the atlas generation as of the end of that row's own shaping.
    fn reshape_row(
        &mut self,
        y: usize,
        width: usize,
        shared: &mut crate::fonts::SharedFontCpuState,
    ) {
        let mut geometry = std::mem::take(&mut self.row_geometry[y]);
        self.shape_row(y, width, shared, &mut geometry);
        geometry.atlas_generation = shared.cached.generation();
        self.row_geometry[y] = geometry;
    }

    /// Forces the next [`Self::take_draw_payload`] call to produce a full
    /// payload (`load_previous == false`) regardless of which rows are
    /// actually marked dirty. Used by `Tui::flush` (setup.rs) when it is
    /// about to overwrite a not-yet-extracted payload - a partial payload
    /// covering only some rows would otherwise lose whatever rows the
    /// overwritten one covered.
    pub(crate) fn force_full_redraw(&mut self) {
        self.full_redraw_needed = true;
    }

    /// This backend's font identity - see [`Fonts::identity`].
    pub(crate) fn font_key(&self) -> usize {
        self.fonts.identity()
    }

    /// Drains this backend's font's SHARED pending glyph rasterizations -
    /// queued via `Fonts::with_shared_cpu_state` by potentially ANY
    /// terminal sharing this font, not just this one. Called once per font
    /// per frame by the render-world extract system (`extract_tui_draws`
    /// in `bevy_plugin.rs`), not once per terminal - whichever terminal
    /// happens to be visited first for a given font in a frame drains
    /// everything queued for it; terminals visited afterward correctly see
    /// an empty result for that same font.
    pub(crate) fn take_shared_glyph_uploads(
        &self,
    ) -> Vec<(crate::utils::text_atlas::CacheRect, Vec<u8>)> {
        self.fonts
            .with_shared_cpu_state(|shared| std::mem::take(&mut shared.pending_cache_updates))
    }

    /// Get the terminal dimensions in characters (columns, rows).
    /// Returns None if the backend is not properly initialized.
    pub fn dimensions(&self) -> Option<(u16, u16)> {
        if self.cols == 0 || self.rows == 0 {
            return None;
        }
        Some((self.cols, self.rows))
    }

    /// Get the text content of the terminal.
    pub fn get_text(&self) -> Vec<Line<'static>> {
        // TODO: Implement text extraction
        vec![]
    }

    /// Update fonts used by the backend.
    pub fn update_fonts(&mut self, new_fonts: Arc<Fonts>) {
        // Invalidate this backend's own caches and mark all dirty. No
        // atlas invalidation needed here: the atlas lives in
        // `Fonts::with_shared_cpu_state`, keyed by the `Fonts` instance
        // itself - switching to a different `Arc<Fonts>` means the next
        // flush looks up (or lazily creates) that OTHER font's own shared
        // state, leaving whatever the outgoing `Fonts` owned untouched
        // (and simply dropped once its last `Arc` clone goes away).
        self.dirty_rows.clear();
        self.row_geometry.clear();
        self.rows_dirty_since_take.clear();
        self.full_redraw_needed = true;
        #[cfg(feature = "ascii_fast_shaping")]
        self.ascii_glyph_cache.clear();
        self.fonts = new_fonts;
    }

    /// Shapes and rasterizes-as-needed a single row (index `y`, of a grid
    /// `width` cells wide) into `out`, replacing its previous contents (its
    /// allocations are reused). Shared by `flush()`'s two passes: a
    /// freshly-dirty row, or a clean row whose cache turned out stale. A
    /// row with no glyphs leaves `out` empty, which is a perfectly valid
    /// cache entry.
    fn shape_row(
        &mut self,
        y: usize,
        width: usize,
        shared: &mut crate::fonts::SharedFontCpuState,
        out: &mut RowGeometry,
    ) {
        use crate::backend::c2c;
        use rustybuzz::shape_with_plan;

        out.cells.clear();
        out.overlays.clear();

        let row_start = y * width;
        let row_end = (row_start + width).min(self.cells.len());
        let row_cells = &self.cells[row_start..row_end];

        // Build row string for shaping
        self.row.clear();
        self.rowmap.clear();
        for (x, cell) in row_cells.iter().enumerate() {
            let symbol = cell.symbol();
            self.row.push_str(symbol);
            // Map each byte to its cell index
            for _ in 0..symbol.len() {
                self.rowmap.push(x as u16);
            }
        }

        if self.row.is_empty() {
            return;
        }
        self.covered.clear();
        self.covered.resize(row_cells.len(), false);

        // One run per stretch of cells resolving to the same font, so each
        // char is shaped by a font that actually has it.
        let runs = font_runs(&self.fonts, row_cells, &mut shared.font_choices);

        let mut glyphs = std::mem::take(&mut self.glyphs);
        glyphs.clear();

        // ASCII fast path (opt-in via the `ascii_fast_shaping` feature -
        // compiled out entirely otherwise, and requires `bold_italic_fonts`
        // to be off, since that feature makes per-cell font selection
        // meaningful while this path assumes the whole row shares one font
        // - which is also why it only runs for a row that `font_runs`
        // resolved to a single run). Synthesizes glyphs straight from the
        // font's cmap instead of invoking rustybuzz, feeding the SAME
        // per-glyph loop below as shaped output. Bails to the shaping path
        // the moment any cell isn't a single ASCII printable byte or the
        // font lacks that glyph.
        #[cfg(all(feature = "ascii_fast_shaping", not(feature = "bold_italic_fonts")))]
        let fast_path = 'fast: {
            use rustybuzz::ttf_parser::Face;

            let [run] = runs.as_slice() else {
                break 'fast false;
            };
            let font = run.font;

            for (x, cell) in row_cells.iter().enumerate() {
                let bytes = cell.symbol().as_bytes();
                // Exactly one ASCII printable byte - this single check also
                // excludes an empty symbol (NULL_CELL wide-char
                // continuation) and any multi-byte UTF-8 grapheme cluster.
                if bytes.len() != 1 || !(0x20..=0x7E).contains(&bytes[0]) {
                    glyphs.clear();
                    break 'fast false;
                }
                let ch = bytes[0] as char;

                let glyph_id = *self
                    .ascii_glyph_cache
                    .entry((font.id(), ch))
                    .or_insert_with(|| Face::glyph_index(font.font(), ch));
                let Some(glyph_id) = glyph_id else {
                    glyphs.clear();
                    break 'fast false; // font lacks this glyph - defer to shaping
                };
                glyphs.push(RowGlyph {
                    glyph_id: glyph_id.0 as u32,
                    cluster: x as u32,
                    x_advance: 0,
                    x_offset: 0,
                    run: 0,
                });
            }
            true
        };
        #[cfg(not(all(feature = "ascii_fast_shaping", not(feature = "bold_italic_fonts"))))]
        let fast_path = false;

        if !fast_path {
            for (run_idx, run) in runs.iter().enumerate() {
                let text = &self.row[run.bytes.clone()];
                let buffer = &mut self.buffer;
                let plan_cache = &mut shared.plan_cache;
                // Shaped once per distinct run text: an unchanged row whose
                // colors moved, or content that merely scrolled to another
                // row, is served from the cache without touching rustybuzz.
                let shaped = shared.shape_cache.get_or_shape(run.font, text, || {
                    let mut unicode = std::mem::take(buffer);
                    unicode.clear();
                    for (idx, ch) in text.char_indices() {
                        unicode.add(ch, idx as u32);
                    }
                    let glyph_buffer = shape_with_plan(
                        run.font.font(),
                        plan_cache.get(run.font, &mut unicode),
                        unicode,
                    );
                    let shaped = glyph_buffer
                        .glyph_infos()
                        .iter()
                        .zip(glyph_buffer.glyph_positions())
                        .map(|(info, pos)| crate::utils::shape_cache::ShapedGlyph {
                            glyph_id: info.glyph_id,
                            cluster: info.cluster,
                            x_advance: pos.x_advance,
                            x_offset: pos.x_offset,
                        })
                        .collect();
                    *buffer = glyph_buffer.clear();
                    shaped
                });
                // Clusters become byte offsets into the whole row - what
                // `rowmap` is indexed by.
                glyphs.extend(shaped.iter().map(|glyph| RowGlyph {
                    glyph_id: glyph.glyph_id,
                    cluster: run.bytes.start as u32 + glyph.cluster,
                    x_advance: glyph.x_advance,
                    x_offset: glyph.x_offset,
                    run: run_idx as u16,
                }));
            }
        }

        let cell_width_px = self.fonts.min_width_px();
        let cell_height_px = self.fonts.height_px();
        let last_resort_id = self.fonts.last_resort_id();

        // Process shaped (or synthesized) glyphs
        for glyph in &glyphs {
            let run = &runs[glyph.run as usize];

            let cluster = glyph.cluster as usize;
            if cluster >= self.rowmap.len() {
                continue;
            }

            let cell_idx = self.rowmap[cluster] as usize;
            if cell_idx >= row_cells.len() {
                continue;
            }

            let cell = &row_cells[cell_idx];

            let (cell_font, cell_fake_bold, cell_fake_italic) =
                (run.font, run.fake_bold, run.fake_italic);

            // Calculate character width using unicode-width for precise glyph width
            use unicode_width::UnicodeWidthChar;
            let ch = cell.symbol().chars().next().unwrap_or(' ');
            let ch_width = ch.width().unwrap_or(1).max(1) as u32;
            let glyph_width_px = ch_width * cell_width_px;

            // Check if this character is an emoji
            #[cfg(feature = "emoji")]
            fn is_emoji(ch: char) -> bool {
                use unicode_properties::UnicodeEmoji;
                // Simplify emoji detection - just check if it's an emoji character
                ch.is_emoji_char()
            }

            #[cfg(not(feature = "emoji"))]
            fn is_emoji(_ch: char) -> bool {
                false
            }

            let is_emoji = is_emoji(ch);

            // Check if this is a programmatic glyph that was pre-rendered
            use crate::backend::programmatic_glyphs::is_programmatic_glyph;
            let is_programmatic = is_programmatic_glyph(ch);

            // Create cache key
            // For programmatic glyphs: use Unicode codepoint + last_resort font (matches populate_programmatic_glyphs)
            // For font glyphs: use shaped glyph ID + actual font

            #[cfg(feature = "bold_italic_fonts")]
            let style =
                cell.modifier & (ratatui::style::Modifier::BOLD | ratatui::style::Modifier::ITALIC);

            #[cfg(not(feature = "bold_italic_fonts"))]
            let style = ratatui::style::Modifier::empty();

            let key = if is_programmatic {
                Key {
                    style,
                    glyph: ch as u32,
                    font: last_resort_id,
                }
            } else {
                Key {
                    style,
                    glyph: glyph.glyph_id,
                    font: cell_font.id(),
                }
            };

            let cached = shared.cached.get(&key, glyph_width_px, cell_height_px);

            // If not cached, render the glyph
            if !cached.cached() {
                if is_programmatic {
                    // Render programmatic glyph on-demand if not pre-cached
                    use crate::backend::programmatic_glyphs::render_programmatic_glyph;

                    if let Some(pixmap) =
                        render_programmatic_glyph(ch, glyph_width_px, cell_height_px)
                    {
                        shared
                            .pending_cache_updates
                            .push((*cached, pixmap_coverage(&pixmap)));
                    } else {
                        tracing::warn!(
                            "Failed to render programmatic glyph '{}' (U+{:04X})",
                            ch,
                            ch as u32
                        );
                    }
                } else {
                    let metrics = cell_font.font();
                    let advance_scale = cell_height_px as f32 / metrics.height() as f32;
                    let fit_x = fit_to_cells(
                        glyph.x_advance as f32 * advance_scale,
                        glyph_width_px as f32,
                    );

                    // Calculate glyph bearing offset to apply during rasterization
                    let bearing_offset_x = glyph.x_offset as f32 * advance_scale * fit_x;

                    let mut info = rustybuzz::GlyphInfo::default();
                    info.glyph_id = glyph.glyph_id;

                    // Don't apply fake styling to emoji characters to avoid distortion
                    let final_fake_italic = cell_fake_italic && !is_emoji;
                    let final_fake_bold = cell_fake_bold && !is_emoji;

                    let (rect, image) = rasterize_glyph(
                        cached,
                        metrics,
                        &info,
                        final_fake_italic, // Don't distort emoji
                        final_fake_bold,   // Don't distort emoji
                        advance_scale,
                        glyph_width_px,   // Use actual glyph width
                        bearing_offset_x, // Apply offset in atlas
                        fit_x,
                    );

                    shared
                        .pending_cache_updates
                        .push((rect, glyph_coverage(&image)));
                }
            }

            // Get colors
            let reverse = cell.modifier.contains(ratatui::style::Modifier::REVERSED);
            // The color actually being used *as the background* -
            // `cell.fg` when reversed, matching the swap below. Checked
            // against `Color::Reset` before `c2c()` resolves it to an
            // opaque RGB, since that resolution is exactly what erases
            // the "this cell has no explicit background" information
            // `transparent_reset_bg` needs.
            let bg_source = if reverse { cell.fg } else { cell.bg };
            let [bg_r, bg_g, bg_b] = if reverse {
                c2c(cell.fg, self.reset_fg)
            } else {
                c2c(cell.bg, self.reset_bg)
            };
            let [fg_r, fg_g, fg_b] = if reverse {
                c2c(cell.bg, self.reset_bg)
            } else {
                c2c(cell.fg, self.reset_fg)
            };
            let bg_alpha =
                if self.transparent_reset_bg && matches!(bg_source, ratatui::style::Color::Reset) {
                    0
                } else {
                    255
                };

            // Aligned to the cell grid (the glyph's bearing is already
            // baked into its atlas slot), sampled 1:1 from the atlas.
            let instance = CellInstance {
                rect: [
                    px_u16(cell_idx as u32 * cell_width_px),
                    px_u16(y as u32 * cell_height_px),
                    px_u16(glyph_width_px),
                    px_u16(cell_height_px),
                ],
                uv: [px_u16(cached.x), px_u16(cached.y)],
                fg: [fg_r, fg_g, fg_b, 255],
                bg: [bg_r, bg_g, bg_b, bg_alpha],
            };
            // The first glyph of a cell writes the whole cell (background
            // included); any further glyph there is blended on top.
            if std::mem::replace(&mut self.covered[cell_idx], true) {
                out.overlays.push(instance);
            } else {
                out.cells.push(instance);
            }
        }

        self.glyphs = glyphs;
    }
}

// ====== Backend trait implementation ======

impl ratatui::backend::Backend for BevyTerminalBackend {
    type Error = std::io::Error;
    fn draw<'a, I>(&mut self, content: I) -> std::io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        use unicode_width::UnicodeWidthStr;

        let bounds = self.size()?;

        self.cells
            .resize(bounds.height as usize * bounds.width as usize, Cell::EMPTY);
        self.dirty_rows.resize(bounds.height as usize, true);
        self.row_geometry
            .resize_with(bounds.height as usize, RowGeometry::default);
        self.cells_changed_last_draw = false;

        for (x, y, cell) in content {
            self.cells_changed_last_draw = true;
            let index = y as usize * bounds.width as usize + x as usize;

            self.cells[index] = cell.clone();

            let width = cell.symbol().width().max(1);
            let start = (index + 1).min(self.cells.len());
            let end = (index + width).min(self.cells.len());
            self.cells[start..end].fill(NULL_CELL);
            self.dirty_rows[y as usize] = true;
        }

        Ok(())
    }

    fn hide_cursor(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    fn show_cursor(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    fn get_cursor_position(&mut self) -> std::io::Result<ratatui::layout::Position> {
        Ok(ratatui::layout::Position::new(self.cursor.0, self.cursor.1))
    }

    fn set_cursor_position<Pos: Into<ratatui::layout::Position>>(
        &mut self,
        position: Pos,
    ) -> std::io::Result<()> {
        let bounds = self.size()?;
        let pos: ratatui::layout::Position = position.into();
        self.cursor = (pos.x.min(bounds.width - 1), pos.y.min(bounds.height - 1));
        Ok(())
    }

    fn clear(&mut self) -> std::io::Result<()> {
        self.cells.clear();
        self.dirty_rows.clear();
        self.row_geometry.clear();
        self.cursor = (0, 0);

        // The destination texture's content is no longer meaningful once
        // the terminal's own cell grid has been cleared - force the next
        // payload to be a full one (`LoadOp::Clear`) rather than a
        // dirty-rows-only partial.
        self.rows_dirty_since_take.clear();
        self.full_redraw_needed = true;

        Ok(())
    }

    fn size(&self) -> std::io::Result<ratatui::layout::Size> {
        let (inset_width, inset_height) = match self.viewport {
            Viewport::Full => (0, 0),
            Viewport::Shrink { width, height } => (width, height),
        };

        // Calculate drawable area based on cols/rows
        let pixel_width = self.cols as u32 * self.fonts.min_width_px();
        let pixel_height = self.rows as u32 * self.fonts.height_px();

        let width = pixel_width - inset_width;
        let height = pixel_height - inset_height;

        Ok(ratatui::layout::Size {
            width: (width / self.fonts.min_width_px()) as u16,
            height: (height / self.fonts.height_px()) as u16,
        })
    }

    fn window_size(&mut self) -> std::io::Result<ratatui::backend::WindowSize> {
        let (inset_width, inset_height) = match self.viewport {
            Viewport::Full => (0, 0),
            Viewport::Shrink { width, height } => (width, height),
        };

        let pixel_width = self.cols as u32 * self.fonts.min_width_px();
        let pixel_height = self.rows as u32 * self.fonts.height_px();

        let width = pixel_width - inset_width;
        let height = pixel_height - inset_height;

        Ok(ratatui::backend::WindowSize {
            columns_rows: ratatui::layout::Size {
                width: (width / self.fonts.min_width_px()) as u16,
                height: (height / self.fonts.height_px()) as u16,
            },
            pixels: ratatui::layout::Size {
                width: width as u16,
                height: height as u16,
            },
        })
    }

    /// Maintain the per-row geometry cache for GPU rendering and
    /// accumulate which rows need re-rendering (`rows_dirty_since_take`,
    /// for partial redraw). Concatenating `row_geometry` into an actual
    /// draw payload - deciding full vs. partial, adding row-clear
    /// instances - happens later, in `take_draw_payload`; this method only
    /// keeps `row_geometry` correct and notes which rows changed.
    ///
    /// Row-incremental, in two steps:
    /// 1. Regenerate every row `dirty_rows` says actually changed, via
    ///    `shape_row`, storing each row's geometry into
    ///    `row_geometry[y]` stamped with the atlas generation as of that
    ///    row's own processing.
    /// 2. For every remaining (clean) row, reuse its cached geometry
    ///    verbatim - UNLESS shaping this frame's dirty rows evicted
    ///    anything from the glyph atlas (`Atlas::generation()` advanced
    ///    since this flush started), in which case a cached row's atlas
    ///    rects might point at a reassigned slot, so it is reshaped too,
    ///    this once. This is a graceful-degradation path: it only engages
    ///    under atlas pressure. Rows reshaped here are NOT added to
    ///    `rows_dirty_since_take` - eviction only invalidates a row's
    ///    cached UVs for a FUTURE redraw of that row; the pixels already
    ///    rendered to the destination texture from its last actual redraw
    ///    remain correct, so no GPU re-render is owed for it this frame.
    ///
    /// A cached row is trustworthy only while nothing has reassigned an
    /// atlas slot since it was recorded: `Atlas::get`'s eviction branch
    /// bumps `Atlas`'s generation counter. Every glyph a row looks up is
    /// stamped as used by the current flush (`Atlas::begin_flush`), and an
    /// eviction only takes such a slot once nothing else is left - so rows
    /// processed THIS flush keep their glyphs; a clean row whose glyphs
    /// weren't looked up is the one at risk, which is exactly what the
    /// generation check guards against.
    ///
    /// Pending glyph rasterizations are stored in `pending_cache_updates` and will be
    /// uploaded to the GPU texture by the render world (see `take_draw_payload`).
    ///
    /// # Performance Notes
    ///
    /// ratatui calls `Backend::flush` unconditionally after every `draw()`,
    /// even when the diff was empty (`Terminal::apply_buffer_with_cursor`
    /// always runs `self.flush()?`). This method early-returns entirely
    /// when the most recent `draw()` didn't actually change any cell -
    /// `cells_changed_last_draw` is exactly that signal (reset at the top
    /// of every `draw()`, set `true` iff the diff yielded at least one
    /// cell). Nothing downstream reads `row_geometry`/`rows_dirty_since_take`
    /// unless `Tui::flush` sees its own `dirty` flag set, which is derived
    /// from this same signal - so skipping here changes nothing observable
    /// on an unchanged frame.
    fn flush(&mut self) -> std::io::Result<()> {
        if !self.cells_changed_last_draw {
            return Ok(());
        }

        let bounds = self.size()?;
        let width = bounds.width as usize;
        let height = bounds.height as usize;

        // `pending_cache_updates` must NOT be cleared here: its entries are
        // one-shot atlas uploads for slots the `Atlas` already considers
        // cached, queued by a *previous* flush (or by
        // `populate_programmatic_glyphs` at creation) and not yet drained
        // by `take_draw_payload`. Clearing them here destroys the only
        // copy of those glyphs' pixels - the glyph is never re-rasterized
        // (the atlas says it's cached) and its slot stays garbage forever,
        // i.e. permanently invisible characters. This is exactly what
        // happens when `Terminal::draw` runs more than once between
        // payload-takes: creation-time populate + a setup-time initial
        // draw + the first per-frame draw are three flushes before the
        // first `gpu_flush_system` ever runs.

        // Track which rows actually changed content this flush,
        // accumulated across flushes since the last `take_draw_payload` -
        // `take` unions this into the row-clear set for the next partial
        // payload. See the field doc on `rows_dirty_since_take` for why
        // pass 2's eviction-driven reshapes below do NOT feed into this.
        self.rows_dirty_since_take.resize(height, false);

        // Snapshot which rows ratatui's diff actually marked dirty coming
        // INTO this flush - `dirty_rows[y]` is cleared as each such row is
        // processed below, so this stable copy is what tells pass 2 which
        // rows still need looking at (a row just cleared by pass 1 must
        // not be reconsidered by pass 2 as "was already clean").
        let was_dirty = self.dirty_rows[..height].to_vec();
        for (y, &dirty) in was_dirty.iter().enumerate() {
            if dirty {
                self.rows_dirty_since_take[y] = true;
            }
        }

        // Cloned (not borrowed) so the closure below can hold it alongside
        // an independent `&mut self` for `self.shape_row(...)` - `self.fonts`
        // itself can't be borrowed for the `with_shared_cpu_state` receiver
        // while a closure captures all of `self` mutably for the `shape_row`
        // calls (the atlas/plan cache live in `Fonts`, shared by every
        // terminal using the same `Arc<Fonts>`).
        let fonts = Arc::clone(&self.fonts);
        fonts.with_shared_cpu_state(|shared| {
            // Every glyph looked up from here on is stamped as in use by
            // this flush - the last thing an atlas eviction will pick.
            shared.cached.begin_flush();
            let generation_before_flush = shared.cached.generation();

            // Pass 1: regenerate every row that actually changed.
            for (y, &dirty) in was_dirty.iter().enumerate() {
                if !dirty {
                    continue;
                }
                self.reshape_row(y, width, shared);
                self.dirty_rows[y] = false;
            }

            // If shaping this frame's dirty rows evicted anything from the
            // atlas, no previously-cached row is safe to reuse blindly this
            // frame (see the doc comment above) - fall back to reshaping
            // every remaining row too, just this once.
            let atlas_evicted_this_flush = shared.cached.generation() != generation_before_flush;

            // Pass 2: rows that were clean coming into this flush.
            for (y, &dirty) in was_dirty.iter().enumerate() {
                if dirty {
                    continue; // already handled in pass 1
                }

                let cache_valid = !atlas_evicted_this_flush
                    && self.row_geometry[y].atlas_generation == generation_before_flush;

                if cache_valid {
                    continue;
                }

                self.reshape_row(y, width, shared);
            }
        });

        Ok(())
    }

    fn clear_region(
        &mut self,
        _clear_type: ratatui::backend::ClearType,
    ) -> Result<(), Self::Error> {
        // Every clear type clears the whole terminal.
        self.clear()
    }
}

// ============================================================================
// Test: cell-level transparency. Pure CPU - drives ratatui's
// `Backend::draw()` directly and inspects `CellInstance::bg`, no GPU/App
// needed: the cell pipeline writes that RGBA as-is wherever the glyph has
// no coverage (`BlendState::REPLACE`).
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts::{Font, Fonts};
    use ratatui::backend::Backend as RatatuiBackend;
    use ratatui::buffer::Cell;
    use ratatui::style::Color;

    fn test_fonts() -> Arc<Fonts> {
        let font_data = include_bytes!("../../examples/assets/fonts/Mplus1Code-Regular.ttf");
        let font = Font::new(font_data).expect("failed to load test font");
        Arc::new(Fonts::new(font, 16))
    }

    #[test]
    fn transparent_reset_bg_zeroes_alpha_only_for_reset_backgrounds() {
        let mut backend = TerminalBuilder::new(test_fonts())
            .with_dimensions(2, 1)
            .with_transparent_reset_bg(true)
            .build();

        let mut reset_cell = Cell::default();
        reset_cell.set_symbol("a");
        reset_cell.bg = Color::Reset;

        let mut colored_cell = Cell::default();
        colored_cell.set_symbol("b");
        colored_cell.bg = Color::Rgb(10, 20, 30);

        RatatuiBackend::draw(
            &mut backend,
            [(0u16, 0u16, &reset_cell), (1u16, 0u16, &colored_cell)].into_iter(),
        )
        .expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");
        let payload = backend.take_draw_payload();

        // First-ever take on this backend is always full (no row-clear
        // instance), so this is exactly one instance per cell.
        assert!(
            payload.is_full(),
            "the first take on a fresh backend must be full"
        );
        assert_eq!(payload.cells().len(), 2, "one instance per cell, 2 cells");
        assert_eq!(
            payload.cells()[0].bg[3],
            0,
            "a Color::Reset background must carry alpha 0 when \
             transparent_reset_bg is enabled"
        );
        assert_eq!(
            payload.cells()[1].bg,
            [10, 20, 30, 255],
            "an explicit background color must stay fully opaque"
        );
    }

    #[test]
    fn transparent_reset_bg_disabled_keeps_reset_cells_opaque() {
        let mut backend = TerminalBuilder::new(test_fonts())
            .with_dimensions(1, 1)
            .with_transparent_reset_bg(false) // the default
            .with_reset_bg([10, 20, 30])
            .build();

        let mut reset_cell = Cell::default();
        reset_cell.set_symbol("a");
        reset_cell.bg = Color::Reset;

        RatatuiBackend::draw(&mut backend, [(0u16, 0u16, &reset_cell)].into_iter())
            .expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");
        let payload = backend.take_draw_payload();

        assert!(!payload.cells().is_empty(), "flush must generate instances");
        for cell in payload.cells() {
            assert_eq!(
                cell.bg,
                [10, 20, 30, 255],
                "without transparent_reset_bg, Reset backgrounds render \
                 opaque (using reset_bg)"
            );
        }
    }

    #[test]
    fn reversed_modifier_checks_fg_for_reset_transparency() {
        // With Modifier::REVERSED, cell.fg becomes the effective background
        // (see the color-swap in flush()) - transparency must follow that
        // swap, not check cell.bg unconditionally.
        let mut backend = TerminalBuilder::new(test_fonts())
            .with_dimensions(1, 1)
            .with_transparent_reset_bg(true)
            .build();

        let mut cell = Cell::default();
        cell.set_symbol("a");
        cell.modifier.insert(ratatui::style::Modifier::REVERSED);
        cell.fg = Color::Reset; // effective background under REVERSED
        cell.bg = Color::Rgb(1, 2, 3); // effective foreground under REVERSED - must not matter

        RatatuiBackend::draw(&mut backend, [(0u16, 0u16, &cell)].into_iter()).expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");
        let payload = backend.take_draw_payload();

        assert!(!payload.cells().is_empty(), "flush must generate instances");
        for cell in payload.cells() {
            assert_eq!(
                cell.bg[3], 0,
                "reversed cell.fg == Reset must still zero the background alpha"
            );
        }
    }

    // ========================================================================
    // Test: flush() early-out on an unchanged frame.
    // ========================================================================

    #[test]
    fn flush_skips_vertex_regeneration_on_byte_identical_redraw() {
        let mut backend = TerminalBuilder::new(test_fonts())
            .with_dimensions(2, 1)
            .build();

        let mut cell = Cell::default();
        cell.set_symbol("a");
        cell.bg = Color::Rgb(10, 20, 30);

        // First draw: content changes (empty -> "a"), so this flush must do
        // real work and produce instances.
        RatatuiBackend::draw(&mut backend, [(0u16, 0u16, &cell)].into_iter()).expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");
        let payload = backend.take_draw_payload();
        assert!(
            !payload.cells().is_empty(),
            "first draw onto an empty buffer must generate instances"
        );

        // Second draw: byte-identical content. Note ratatui's own
        // `Terminal::flush` (ratatui-core `terminal/buffers.rs`) is what
        // diffs the previous/current buffers and passes only the CHANGED
        // cells to `Backend::draw` - an identical redraw means that diff is
        // empty, so `Backend::draw` is called with an empty iterator (not
        // the same cell tuple again; feeding the same tuple twice would
        // incorrectly mark it changed both times, since this trait method
        // itself performs no diffing - it trusts the caller already diffed).
        RatatuiBackend::draw(&mut backend, std::iter::empty()).expect("draw failed");
        assert!(
            !backend.cells_changed_last_draw(),
            "an identical redraw (empty diff) must not report a cell change"
        );
        RatatuiBackend::flush(&mut backend).expect("flush failed");

        let payload = backend.take_draw_payload();
        assert!(
            payload.cells().is_empty() && payload.overlays().is_empty(),
            "flush must skip regenerating instances for an unchanged frame"
        );
    }

    // ========================================================================
    // Test: row-incremental flush. Instance order within a row is not part
    // of the contract, so instances are compared as an order-independent
    // multiset via their raw bytes (`CellInstance` is `bytemuck::Pod`, so
    // this is a byte-exact comparison, not a fuzzy one).
    // ========================================================================

    fn sorted_bytes<T: bytemuck::Pod>(items: &[T]) -> Vec<Vec<u8>> {
        let mut bytes: Vec<Vec<u8>> = items
            .iter()
            .map(|i| bytemuck::bytes_of(i).to_vec())
            .collect();
        bytes.sort();
        bytes
    }

    #[test]
    fn row_incremental_flush_matches_a_from_scratch_flush_of_the_same_content() {
        // Deliberately a SINGLE backend (one `Atlas`) throughout - a glyph's
        // UV rect depends on its insertion order into the atlas, not just
        // its identity, so two independently-built backends would assign
        // "A"/"B"/"C"/"D"/"Z" to different slots and legitimately produce
        // different (but each internally-correct) UV bytes. Comparing across
        // two atlases would be testing atlas insertion order, not row-cache
        // correctness. Instead: incrementally flush on this backend, save
        // the result, then force the SAME backend (same atlas, same
        // established slot assignments - no new glyphs are introduced by
        // this step, so no new allocation) through a full reshape of every
        // row and compare against the saved result.
        fn cell_with_symbol(symbol: &str) -> Cell {
            let mut cell = Cell::default();
            cell.set_symbol(symbol);
            cell
        }

        let mut backend = TerminalBuilder::new(test_fonts())
            .with_dimensions(1, 4)
            .build();

        let row_a = cell_with_symbol("A");
        let row_b = cell_with_symbol("B");
        let row_c = cell_with_symbol("C");
        let row_d = cell_with_symbol("D");
        RatatuiBackend::draw(
            &mut backend,
            [
                (0u16, 0u16, &row_a),
                (0u16, 1u16, &row_b),
                (0u16, 2u16, &row_c),
                (0u16, 3u16, &row_d),
            ]
            .into_iter(),
        )
        .expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");
        let _ = backend.take_draw_payload();

        // Change ONLY row 2's cell and flush again - the row cache should
        // reuse rows 0, 1, 3 from `row_geometry` and reshape only row 2
        // (allocating "Z" a fresh atlas slot).
        let row_c_changed = cell_with_symbol("Z");
        RatatuiBackend::draw(&mut backend, [(0u16, 2u16, &row_c_changed)].into_iter())
            .expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");

        // Snapshot the row cache produced incrementally. `row_geometry` is
        // what this test verifies; `take_draw_payload`'s full/partial
        // concatenation and row-clear instances are orthogonal to
        // row-cache correctness, so comparing `row_geometry` directly
        // (rather than a concatenated payload) keeps this test decoupled
        // from that concern.
        let incremental_rows: Vec<RowGeometry> = backend.row_geometry.clone();
        let _ = backend.take_draw_payload();

        // Force every row to be reshaped from scratch, on this same
        // backend. "A", "B", "Z", "D" are all already in the atlas from
        // the two flushes above, so this introduces no new glyphs and
        // therefore no new allocations - `Atlas::get` hits the existing
        // entries and returns the SAME atlas rects it already handed out,
        // making this a true apples-to-apples comparison against the
        // incremental flush above.
        backend
            .dirty_rows
            .iter_mut()
            .for_each(|dirty| *dirty = true);
        backend.cells_changed_last_draw = true;
        RatatuiBackend::flush(&mut backend).expect("flush failed");

        for (y, (incremental, reshaped)) in incremental_rows
            .iter()
            .zip(backend.row_geometry.iter())
            .enumerate()
        {
            assert_eq!(
                sorted_bytes(&incremental.cells),
                sorted_bytes(&reshaped.cells),
                "row {y} cell instances from an incremental (row-cached) flush must \
                 match a full reshape of the same content on the same atlas state"
            );
            assert_eq!(
                sorted_bytes(&incremental.overlays),
                sorted_bytes(&reshaped.overlays),
                "row {y} overlay instances from an incremental (row-cached) flush must \
                 match a full reshape of the same content on the same atlas state"
            );
        }
    }

    // ========================================================================
    // Tests: partial redraw (full vs. dirty-rows-only payloads).
    // ========================================================================

    #[test]
    fn first_take_is_full() {
        let mut backend = TerminalBuilder::new(test_fonts())
            .with_dimensions(2, 1)
            .build();

        let mut cell = Cell::default();
        cell.set_symbol("a");
        cell.bg = Color::Rgb(10, 20, 30);

        RatatuiBackend::draw(&mut backend, [(0u16, 0u16, &cell)].into_iter()).expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");
        let payload = backend.take_draw_payload();

        assert!(
            payload.is_full(),
            "the very first take must be a full payload"
        );
    }

    #[test]
    fn partial_take_contains_only_dirty_rows() {
        fn cell_with_symbol(symbol: &str) -> Cell {
            let mut cell = Cell::default();
            cell.set_symbol(symbol);
            cell
        }

        let mut backend = TerminalBuilder::new(test_fonts())
            .with_dimensions(1, 3)
            .build();

        let row_a = cell_with_symbol("A");
        let row_b = cell_with_symbol("B");
        let row_c = cell_with_symbol("C");
        RatatuiBackend::draw(
            &mut backend,
            [
                (0u16, 0u16, &row_a),
                (0u16, 1u16, &row_b),
                (0u16, 2u16, &row_c),
            ]
            .into_iter(),
        )
        .expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");
        let _ = backend.take_draw_payload(); // consume the initial full take

        // Change only row 1.
        let row_b_changed = cell_with_symbol("Z");
        RatatuiBackend::draw(&mut backend, [(0u16, 1u16, &row_b_changed)].into_iter())
            .expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");
        let payload = backend.take_draw_payload();

        assert!(
            !payload.is_full(),
            "a single-row change must take a partial payload"
        );

        let cell_width_px = backend.fonts.min_width_px() as u16;
        let cell_height_px = backend.fonts.height_px() as u16;

        // The first instance is the row-clear rectangle for row 1: the
        // whole row, in the `initial_fill` color, glyph coverage off.
        let clear = payload.cells()[0];
        assert_eq!(
            clear.rect,
            [0, cell_height_px, cell_width_px, cell_height_px]
        );
        assert_eq!(clear.bg, backend.initial_fill);
        assert_eq!(clear.fg[3], 0, "a row-clear instance must not draw a glyph");

        // No instance (row clear or actual content) may lie outside row
        // 1's pixel span - partial redraw must not touch rows it didn't
        // mark dirty.
        assert_eq!(payload.cells().len(), 2, "row clear + the one changed cell");
        for instance in payload.cells().iter().chain(payload.overlays()) {
            assert_eq!(instance.rect[1], cell_height_px);
            assert_eq!(instance.rect[3], cell_height_px);
        }
    }

    #[test]
    fn partial_take_row_clear_quad_respects_alpha_zero_initial_fill() {
        let mut backend = TerminalBuilder::new(test_fonts())
            .with_dimensions(1, 2)
            .with_initial_fill([0, 0, 0, 0]) // transparent_reset_bg-style fill
            .build();

        let mut cell_top = Cell::default();
        cell_top.set_symbol("A");
        let mut cell_bottom = Cell::default();
        cell_bottom.set_symbol("B");

        RatatuiBackend::draw(
            &mut backend,
            [(0u16, 0u16, &cell_top), (0u16, 1u16, &cell_bottom)].into_iter(),
        )
        .expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");
        let _ = backend.take_draw_payload();

        let mut cell_bottom_changed = Cell::default();
        cell_bottom_changed.set_symbol("Z");
        RatatuiBackend::draw(
            &mut backend,
            [(0u16, 1u16, &cell_bottom_changed)].into_iter(),
        )
        .expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");
        let payload = backend.take_draw_payload();

        assert!(!payload.is_full());
        assert_eq!(
            payload.cells()[0].bg[3],
            0,
            "the row-clear instance must carry initial_fill's alpha (0), \
             matching what a LoadOp::Clear(initial_fill) would have produced"
        );
    }

    #[test]
    fn take_after_resize_is_full() {
        let mut backend = TerminalBuilder::new(test_fonts())
            .with_dimensions(1, 1)
            .build();

        let mut cell = Cell::default();
        cell.set_symbol("a");
        RatatuiBackend::draw(&mut backend, [(0u16, 0u16, &cell)].into_iter()).expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");
        let _ = backend.take_draw_payload();

        backend.resize(2, 2);
        RatatuiBackend::draw(&mut backend, [(0u16, 0u16, &cell)].into_iter()).expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");
        let payload = backend.take_draw_payload();

        assert!(
            payload.is_full(),
            "the first take after a resize must be full"
        );
    }

    #[test]
    fn take_after_clear_is_full() {
        let mut backend = TerminalBuilder::new(test_fonts())
            .with_dimensions(1, 1)
            .build();

        let mut cell = Cell::default();
        cell.set_symbol("a");
        RatatuiBackend::draw(&mut backend, [(0u16, 0u16, &cell)].into_iter()).expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");
        let _ = backend.take_draw_payload();

        RatatuiBackend::clear(&mut backend).expect("clear failed");
        RatatuiBackend::draw(&mut backend, [(0u16, 0u16, &cell)].into_iter()).expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");
        let payload = backend.take_draw_payload();

        assert!(
            payload.is_full(),
            "the first take after a clear must be full"
        );
    }

    #[test]
    fn all_rows_dirty_takes_full_path() {
        fn cell_with_symbol(symbol: &str) -> Cell {
            let mut cell = Cell::default();
            cell.set_symbol(symbol);
            cell
        }

        let mut backend = TerminalBuilder::new(test_fonts())
            .with_dimensions(1, 2)
            .build();

        let row_a = cell_with_symbol("A");
        let row_b = cell_with_symbol("B");
        RatatuiBackend::draw(
            &mut backend,
            [(0u16, 0u16, &row_a), (0u16, 1u16, &row_b)].into_iter(),
        )
        .expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");
        let _ = backend.take_draw_payload(); // consume the initial full take

        // Change every row - `all_dirty` should route this back through the
        // full path (cheaper than a row-clear instance per row), even
        // though `full_redraw_needed` is false at this point.
        let row_a2 = cell_with_symbol("C");
        let row_b2 = cell_with_symbol("D");
        RatatuiBackend::draw(
            &mut backend,
            [(0u16, 0u16, &row_a2), (0u16, 1u16, &row_b2)].into_iter(),
        )
        .expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");
        let payload = backend.take_draw_payload();

        assert!(payload.is_full(), "every row dirty must take the full path");
    }
}
