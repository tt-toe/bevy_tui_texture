use std::sync::Arc;

use crate::backend::CellInstance;
use crate::backend::Viewport;
use crate::backend::glyph_constraint::{CellClass, GlyphConstraint};
use crate::backend::rasterize::{Placement, rasterize_glyph};
use crate::colors::Rgb;
use crate::fonts::Fonts;
use crate::utils::text_atlas::Key;
use ratatui::buffer::Cell;
use ratatui::text::Line;
use rustybuzz::UnicodeBuffer;

const NULL_CELL: Cell = Cell::new("");

/// Cached geometry for one row, from the last flush that (re)generated it.
#[derive(Default, Clone)]
struct RowGeometry {
    /// One instance per covered cell - see [`CellInstance`].
    cells: Vec<CellInstance>,
    /// Extra glyphs landing in a cell `cells` already covers (combining
    /// marks shaped as separate glyphs).
    overlays: Vec<CellInstance>,
    /// Color glyphs (emoji), drawn by their own pipeline after `cells`.
    #[cfg(feature = "emoji")]
    colors: Vec<CellInstance>,
    /// `SharedFontCpuState::atlas_generation()` as of when this geometry was
    /// (re)computed.
    atlas_generation: u64,
}

/// One glyph of the row being shaped: a [`ShapedGlyph`] rebased onto the
/// row (its cluster is a byte offset into the whole row string) and tagged
/// with the [`FontRun`] it came from.
#[derive(Clone, Copy)]
struct RowGlyph {
    glyph_id: u32,
    cluster: u32,
    x_advance: i32,
    x_offset: i32,
    run: u16,
}

/// A maximal stretch of a row whose cells all resolve to the same font
/// (and fake styling), shaped as one rustybuzz run.
struct FontRun<'f> {
    font: &'f crate::fonts::Font,
    fake_bold: bool,
    fake_italic: bool,
    /// Whether any of its cells is a symbol (see
    /// [`crate::backend::glyph_constraint`]) - known for free from the
    /// font choice, and false for practically every run of text, which
    /// then never asks again per cell.
    symbols: bool,
    /// Whether the font is an emoji font (`Fonts::add_emoji_fonts`): its
    /// color glyphs go to the color atlas.
    #[cfg(feature = "emoji")]
    color: bool,
    bytes: std::ops::Range<usize>,
}

/// Splits a row into [`FontRun`]s via [`Fonts::font_for_cell`].
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
        let (font, fake_bold, fake_italic, class) = fonts.font_for_cell_cached(cell, choices);
        let styled = cfg!(feature = "bold_italic_fonts");
        let (fake_bold, fake_italic) = (styled && fake_bold, styled && fake_italic);
        match runs.last_mut() {
            Some(run)
                if run.font.id() == font.id()
                    && run.fake_bold == fake_bold
                    && run.fake_italic == fake_italic =>
            {
                run.bytes.end = end;
                run.symbols |= class.is_symbol();
            }
            _ => runs.push(FontRun {
                font,
                fake_bold,
                fake_italic,
                symbols: class.is_symbol(),
                #[cfg(feature = "emoji")]
                color: fonts.is_emoji_font(font.id()),
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

    #[test]
    fn chars_missing_from_the_first_font_get_a_fallback_run() {
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

/// Shapes `text` with `font` through rustybuzz.
fn shape_text(
    font: &crate::fonts::Font,
    text: &str,
    buffer: &mut UnicodeBuffer,
    plan_cache: &mut crate::utils::plan_cache::PlanCache,
) -> Vec<crate::utils::shape_cache::ShapedGlyph> {
    let mut unicode = std::mem::take(buffer);
    unicode.clear();
    for (idx, ch) in text.char_indices() {
        unicode.add(ch, idx as u32);
    }
    let glyph_buffer =
        rustybuzz::shape_with_plan(font.font(), plan_cache.get(font, &mut unicode), unicode);
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
}

/// Horizontal squeeze for a glyph whose advance (`advance_px`) is wider
/// than the cells it occupies (`cells_px`).
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
        assert_eq!(fit_to_cells(0.0, 6.0), 1.0);
    }

    #[test]
    fn a_wider_fallback_glyph_is_squeezed_to_its_cells() {
        assert_eq!(fit_to_cells(8.0, 6.0), 0.75);
        assert_eq!(fit_to_cells(16.0, 12.0), 0.75);
    }
}

/// A ratatui backend optimized for Bevy integration.
pub struct BevyTerminalBackend {
    pub(super) cols: u16,
    pub(super) rows: u16,

    pub(super) cells: Vec<Cell>,
    pub(super) dirty_rows: Vec<bool>,
    /// Set by `draw()` when ratatui's internal buffer diff contained at
    /// least one changed cell; cleared at the start of every `draw()` call.
    pub(super) cells_changed_last_draw: bool,
    pub(super) cursor: (u16, u16),
    pub(super) viewport: Viewport,
    /// Rows needing re-render, accumulated across flushes since the last
    /// `take_draw_payload`: each `flush()` unions ratatui's dirty rows in;
    /// `take_draw_payload` consumes and clears it.
    pub(super) rows_dirty_since_take: Vec<bool>,
    /// True whenever the destination texture's current content can't be
    /// trusted to already hold everything up to the last taken payload
    /// (just created / resized / cleared / fonts swapped, or a main-world
    /// pending-payload overwrite - see `Tui::flush` and
    /// `Tui::apply_pending_resize`).
    pub(super) full_redraw_needed: bool,

    pub(super) fonts: Arc<Fonts>,
    pub(super) buffer: UnicodeBuffer,
    pub(super) row: String,
    pub(super) rowmap: Vec<u16>,
    /// Scratch for `shape_row`: what each cell of the row being shaped has
    /// drawn so far (a further glyph in a drawn cell becomes an overlay).
    covered: Vec<Cover>,
    /// Scratch for `shape_row`: the row's glyphs, in shaping order.
    glyphs: Vec<RowGlyph>,

    /// Per-row geometry cache, indexed by row `y`, letting `flush()` skip
    /// reshaping+re-rasterizing rows `dirty_rows` says are unchanged.
    row_geometry: Vec<RowGeometry>,

    pub(super) reset_fg: Rgb,
    pub(super) reset_bg: Rgb,
    /// If true, a cell whose *effective* background is `Color::Reset`
    /// (accounting for `Modifier::REVERSED` - see the color-selection logic
    /// in `shape_row`) gets background alpha 0 instead of 255, making it
    /// show through to whatever is behind the terminal's surface (a
    /// world-quad with `AlphaMode::Blend`, or a transparent-background UI
    /// node).
    pub(super) transparent_reset_bg: bool,
    /// Color shown before any content has been drawn (or while nothing is
    /// drawn at all - e.g. mid-resize): the render world's `LoadOp::Clear`
    /// color whenever a draw payload's vertex data is empty.
    pub(super) initial_fill: [u8; 4],
}

/// Builder for BevyTerminalBackend.
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
            reset_fg: [255, 255, 255],
            reset_bg: [0, 0, 0],
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
    /// `BevyTerminalBackend::transparent_reset_bg`.
    pub fn with_transparent_reset_bg(mut self, transparent: bool) -> Self {
        self.transparent_reset_bg = transparent;
        self
    }

    /// Color shown before any content has been drawn.
    pub fn with_initial_fill(mut self, initial_fill: [u8; 4]) -> Self {
        self.initial_fill = initial_fill;
        self
    }

    /// Build the BevyTerminalBackend.
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

/// Pixel coordinate as the `u16` a [`CellInstance`] stores.
fn px_u16(px: u32) -> u16 {
    px.min(u16::MAX as u32) as u16
}

/// What `shape_row` has drawn into a cell so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cover {
    /// Nothing yet: the cell's first glyph writes it outright.
    Free,
    /// A glyph: further glyphs there are overlays.
    Glyph,
    /// The right half of the symbol to its left, spread over this blank
    /// cell: the cell's own (blank) glyph is not drawn.
    SymbolTail,
    /// A color glyph, in the color atlas: further glyphs there (the rest of
    /// an unligated ZWJ sequence) are dropped.
    #[cfg(feature = "emoji")]
    Color,
}

/// A cell's foreground and background as instance colors.
fn cell_colors(
    cell: &Cell,
    reset_fg: Rgb,
    reset_bg: Rgb,
    transparent_reset_bg: bool,
) -> ([u8; 4], [u8; 4]) {
    use crate::backend::c2c;
    let reverse = cell.modifier.contains(ratatui::style::Modifier::REVERSED);
    let bg_source = if reverse { cell.fg } else { cell.bg };
    let [bg_r, bg_g, bg_b] = if reverse {
        c2c(cell.fg, reset_fg)
    } else {
        c2c(cell.bg, reset_bg)
    };
    let [fg_r, fg_g, fg_b] = if reverse {
        c2c(cell.bg, reset_bg)
    } else {
        c2c(cell.fg, reset_fg)
    };
    let bg_alpha = if transparent_reset_bg && matches!(bg_source, ratatui::style::Color::Reset) {
        0
    } else {
        255
    };
    ([fg_r, fg_g, fg_b, 255], [bg_r, bg_g, bg_b, bg_alpha])
}

/// The styling part of a glyph's atlas key: bold/italic only matter with
/// real per-style fonts.
fn key_style(cell: &Cell) -> ratatui::style::Modifier {
    #[cfg(feature = "bold_italic_fonts")]
    let style = cell.modifier & (ratatui::style::Modifier::BOLD | ratatui::style::Modifier::ITALIC);
    #[cfg(not(feature = "bold_italic_fonts"))]
    let style = {
        let _ = cell;
        ratatui::style::Modifier::empty()
    };
    style
}

/// What [`draw_symbol`] (and `draw_color_glyph`, with `emoji`) needs to know
/// about the row being shaped.
struct SymbolRow<'a> {
    fonts: &'a Fonts,
    cells: &'a [Cell],
    y: usize,
    /// `reset_fg`, `reset_bg`, `transparent_reset_bg` - see [`cell_colors`].
    colors: (Rgb, Rgb, bool),
}

/// Draws the first glyph of cell `x` if the cell is a symbol (see
/// [`crate::backend::glyph_constraint`]), returning whether it was: fitted
/// to the grid by its constraint, across two cells when [`symbol_span`]
/// allows it.
#[inline(never)]
fn draw_symbol(
    row: &SymbolRow,
    glyph: &RowGlyph,
    font: &crate::fonts::Font,
    x: usize,
    covered: &mut [Cover],
    shared: &mut crate::fonts::SharedFontCpuState,
    out: &mut RowGeometry,
) -> bool {
    use crate::backend::glyph_constraint::{is_blank, symbol_span};
    use unicode_width::UnicodeWidthChar;

    let mut class_of = |cell: &Cell| match cell.symbol() {
        "" => CellClass::TEXT,
        _ => {
            row.fonts
                .font_for_cell_cached(cell, &mut shared.font_choices)
                .3
        }
    };
    let cell = &row.cells[x];
    let class = class_of(cell);
    if !class.is_symbol() {
        return false;
    }
    let ch = cell.symbol().chars().next().unwrap_or(' ');
    let cell_width_px = row.fonts.min_width_px();
    let cell_height_px = row.fonts.height_px();

    let span = match ch.width().unwrap_or(1).max(1) as u32 {
        1 => {
            let blank_next = row
                .cells
                .get(x + 1)
                .is_some_and(|next| is_blank(next.symbol()))
                && covered[x + 1] == Cover::Free;
            let prev = x.checked_sub(1).map(|prev| class_of(&row.cells[prev]));
            symbol_span(class, prev, blank_next)
        }
        wide => wide,
    };
    let spreads = span == 2 && ch.width() == Some(1);
    let width_px = span * cell_width_px;

    let key = Key {
        style: key_style(cell),
        glyph: glyph.glyph_id,
        font: font.id(),
        cells: span as u8,
    };
    let cached = shared.cached.get(&key, width_px, cell_height_px);
    if !cached.cached() {
        let face = font.font();
        let mut info = rustybuzz::GlyphInfo::default();
        info.glyph_id = glyph.glyph_id;
        let (rect, image) = rasterize_glyph(
            cached,
            face,
            &info,
            false,
            false,
            cell_height_px as f32 / face.height() as f32,
            Placement::Constrained {
                constraint: GlyphConstraint::for_symbol(ch),
                metrics: row.fonts.cell_metrics(),
                cells: span,
            },
        );
        shared
            .pending_cache_updates
            .push((rect, glyph_coverage(&image)));
    }

    let (reset_fg, reset_bg, transparent) = row.colors;
    let (fg, bg) = cell_colors(cell, reset_fg, reset_bg, transparent);
    let x_px = x as u32 * cell_width_px;
    let instance = |x_px: u32, width_px: u32, u: u32, bg: [u8; 4]| CellInstance {
        rect: [
            px_u16(x_px),
            px_u16(row.y as u32 * cell_height_px),
            px_u16(width_px),
            px_u16(cell_height_px),
        ],
        uv: [px_u16(u), px_u16(cached.y)],
        fg,
        bg,
    };
    covered[x] = Cover::Glyph;
    if !spreads {
        out.cells.push(instance(x_px, width_px, cached.x, bg));
        return true;
    }
    let (_, tail_bg) = cell_colors(&row.cells[x + 1], reset_fg, reset_bg, transparent);
    if tail_bg == bg {
        out.cells.push(instance(x_px, width_px, cached.x, bg));
    } else {
        out.cells.push(instance(x_px, cell_width_px, cached.x, bg));
        out.cells.push(instance(
            x_px + cell_width_px,
            cell_width_px,
            cached.x + cell_width_px,
            tail_bg,
        ));
    }
    covered[x + 1] = Cover::SymbolTail;
    true
}

/// Whether `c` is a combining mark to draw over a color glyph.
#[cfg(feature = "emoji")]
fn is_overlay_mark(c: char) -> bool {
    use unicode_properties::{GeneralCategoryGroup, UnicodeGeneralCategory};
    c.general_category_group() == GeneralCategoryGroup::Mark
        && !matches!(c, '\u{20E3}' | '\u{FE00}'..='\u{FE0F}' | '\u{E0100}'..='\u{E01EF}')
}

/// Draws the first glyph of cell `x` into the color atlas if its font has
/// color data for it (COLR, or a PNG or BGRA strike), returning whether the
/// glyph was handled - the caller then skips it.
#[cfg(feature = "emoji")]
#[inline(never)]
fn draw_color_glyph(
    row: &SymbolRow,
    glyph: &RowGlyph,
    font: &crate::fonts::Font,
    x: usize,
    covered: &mut [Cover],
    shared: &mut crate::fonts::SharedFontCpuState,
    out: &mut RowGeometry,
) -> bool {
    use crate::backend::rasterize::rasterize_color_glyph;
    use ratatui::style::Modifier;
    use rustybuzz::ttf_parser::{GlyphId, RasterImageFormat};
    use unicode_width::UnicodeWidthStr;

    if covered[x] == Cover::Color {
        return true;
    }
    if covered[x] != Cover::Free {
        return false;
    }
    let face = font.font();
    let id = GlyphId(glyph.glyph_id as u16);
    let has_color = face.is_color_glyph(id)
        || face.glyph_raster_image(id, u16::MAX).is_some_and(|raster| {
            matches!(
                raster.format,
                RasterImageFormat::PNG | RasterImageFormat::BitmapPremulBgra32
            )
        });
    if !has_color {
        return false;
    }

    let cell = &row.cells[x];
    let cell_width_px = row.fonts.min_width_px();
    let cell_height_px = row.fonts.height_px();
    let cells = cell.symbol().width().clamp(1, 2) as u32;
    let width_px = cells * cell_width_px;

    let key = Key {
        style: Modifier::empty(),
        glyph: glyph.glyph_id,
        font: font.id(),
        cells: cells as u8,
    };
    let cached = shared.color.get(&key, width_px, cell_height_px);
    if !cached.cached() {
        let image = rasterize_color_glyph(face, id, width_px, cell_height_px)
            .unwrap_or_else(|| vec![0; (width_px * cell_height_px * 4) as usize]);
        shared.pending_color_updates.push((*cached, image));
    }

    let (reset_fg, reset_bg, transparent) = row.colors;
    let (fg, bg) = cell_colors(cell, reset_fg, reset_bg, transparent);
    let x_px = x as u32 * cell_width_px;
    let y_px = row.y as u32 * cell_height_px;
    out.colors.push(CellInstance {
        rect: [
            px_u16(x_px),
            px_u16(y_px),
            px_u16(width_px),
            px_u16(cell_height_px),
        ],
        uv: [px_u16(cached.x), px_u16(cached.y)],
        fg: [0; 4],
        bg,
    });
    covered[x] = Cover::Color;

    for mark in cell
        .symbol()
        .chars()
        .skip(1)
        .filter(|&c| is_overlay_mark(c))
    {
        let Some(mark_font) = row.fonts.text_font_with(mark) else {
            continue;
        };
        let mark_face = mark_font.font();
        let Some(mark_id) = mark_face.glyph_index(mark) else {
            continue;
        };
        let key = Key {
            style: Modifier::empty(),
            glyph: mark_id.0 as u32,
            font: mark_font.id(),
            cells: 1,
        };
        let cached = shared.cached.get(&key, cell_width_px, cell_height_px);
        if !cached.cached() {
            let mut info = rustybuzz::GlyphInfo::default();
            info.glyph_id = mark_id.0 as u32;
            let (rect, image) = rasterize_glyph(
                cached,
                mark_face,
                &info,
                false,
                false,
                cell_height_px as f32 / mark_face.height() as f32,
                Placement::Natural {
                    bearing_offset_x: 0.0,
                    fit_x: 1.0,
                },
            );
            shared
                .pending_cache_updates
                .push((rect, glyph_coverage(&image)));
        }
        out.overlays.push(CellInstance {
            rect: [
                px_u16(x_px + (cells - 1) * cell_width_px / 2),
                px_u16(y_px),
                px_u16(cell_width_px),
                px_u16(cell_height_px),
            ],
            uv: [px_u16(cached.x), px_u16(cached.y)],
            fg,
            bg,
        });
    }
    true
}

impl BevyTerminalBackend {
    /// Whether the most recent `draw()` call's ratatui buffer diff touched
    /// at least one cell.
    pub(crate) fn cells_changed_last_draw(&self) -> bool {
        self.cells_changed_last_draw
    }

    /// Update the grid dimensions used by `Backend::size()`/`window_size()`.
    pub(crate) fn resize(&mut self, cols: u16, rows: u16) {
        self.cols = cols;
        self.rows = rows;

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
            let pixmap = match render_programmatic_glyph(unicode_char, width, height) {
                Some(p) => p,
                None => {
                    skipped_count += 1;
                    continue;
                }
            };

            let bitmap = pixmap_coverage(&pixmap);

            let key = Key {
                style: Modifier::empty(),
                glyph: unicode_char as u32,
                font: font_id,
                cells: 1,
            };

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

        let redraw = |y: usize| full || self.rows_dirty_since_take.get(y).copied().unwrap_or(false);
        let rows = || {
            (0..height)
                .filter(|&y| redraw(y))
                .filter_map(|y| self.row_geometry.get(y).map(|row| (y, row)))
        };

        let clears = if full {
            0
        } else {
            (0..height).filter(|&y| redraw(y)).count()
        };
        let cell_count = clears + rows().map(|(_, row)| row.cells.len()).sum::<usize>();
        let overlay_count = rows().map(|(_, row)| row.overlays.len()).sum::<usize>();
        #[cfg(feature = "emoji")]
        let color_count = rows().map(|(_, row)| row.colors.len()).sum::<usize>();
        #[cfg(not(feature = "emoji"))]
        let color_count = 0;
        let mut instances = Vec::with_capacity(cell_count + color_count + overlay_count);

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
        #[cfg(feature = "emoji")]
        if color_count > 0 {
            for (_, row) in rows() {
                instances.extend_from_slice(&row.colors);
            }
        }
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
            #[cfg(feature = "emoji")]
            color_count: color_count as u32,
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
        geometry.atlas_generation = shared.atlas_generation();
        self.row_geometry[y] = geometry;
    }

    /// Forces the next [`Self::take_draw_payload`] call to produce a full
    /// payload (`load_previous == false`) regardless of which rows are
    /// actually marked dirty.
    pub(crate) fn force_full_redraw(&mut self) {
        self.full_redraw_needed = true;
    }

    /// This backend's font identity - see [`Fonts::identity`].
    pub(crate) fn font_key(&self) -> usize {
        self.fonts.identity()
    }

    /// Drains this backend's font's SHARED pending glyph rasterizations -
    /// queued via `Fonts::with_shared_cpu_state` by potentially ANY
    /// terminal sharing this font, not just this one.
    pub(crate) fn take_shared_glyph_uploads(
        &self,
    ) -> Vec<(crate::utils::text_atlas::CacheRect, Vec<u8>)> {
        self.fonts
            .with_shared_cpu_state(|shared| std::mem::take(&mut shared.pending_cache_updates))
    }

    /// [`Self::take_shared_glyph_uploads`] for the color atlas: its size,
    /// and the queued color glyph rasterizations (premultiplied RGBA8).
    #[cfg(feature = "emoji")]
    pub(crate) fn take_shared_color_uploads(
        &self,
    ) -> (
        (u32, u32),
        Vec<(crate::utils::text_atlas::CacheRect, Vec<u8>)>,
    ) {
        self.fonts.with_shared_cpu_state(|shared| {
            (
                shared.color_size,
                std::mem::take(&mut shared.pending_color_updates),
            )
        })
    }

    /// Get the terminal dimensions in characters (columns, rows).
    pub fn dimensions(&self) -> Option<(u16, u16)> {
        if self.cols == 0 || self.rows == 0 {
            return None;
        }
        Some((self.cols, self.rows))
    }

    /// Get the text content of the terminal.
    pub fn get_text(&self) -> Vec<Line<'static>> {
        vec![]
    }

    /// Update fonts used by the backend.
    pub fn update_fonts(&mut self, new_fonts: Arc<Fonts>) {
        self.dirty_rows.clear();
        self.row_geometry.clear();
        self.rows_dirty_since_take.clear();
        self.full_redraw_needed = true;
        self.fonts = new_fonts;
    }

    /// Shapes and rasterizes-as-needed a single row (index `y`, of a grid
    /// `width` cells wide) into `out`, replacing its previous contents (its
    /// allocations are reused).
    fn shape_row(
        &mut self,
        y: usize,
        width: usize,
        shared: &mut crate::fonts::SharedFontCpuState,
        out: &mut RowGeometry,
    ) {
        out.cells.clear();
        out.overlays.clear();
        #[cfg(feature = "emoji")]
        out.colors.clear();

        let row_start = y * width;
        let row_end = (row_start + width).min(self.cells.len());
        let row_cells = &self.cells[row_start..row_end];

        let per_cell = self.fonts.shaping() == crate::fonts::Shaping::Cell;
        self.row.clear();
        self.rowmap.clear();
        for (x, cell) in row_cells.iter().enumerate() {
            let symbol = cell.symbol();
            if !per_cell {
                self.row.push_str(symbol);
            }
            for _ in 0..symbol.len() {
                self.rowmap.push(x as u16);
            }
        }

        if self.rowmap.is_empty() {
            return;
        }
        self.covered.clear();
        self.covered.resize(row_cells.len(), Cover::Free);

        let runs = font_runs(&self.fonts, row_cells, &mut shared.font_choices);

        let mut glyphs = std::mem::take(&mut self.glyphs);
        glyphs.clear();

        if per_cell {
            let mut cells = row_cells.iter();
            let mut offset = 0;
            for (run_idx, run) in runs.iter().enumerate() {
                let shapes = shared.cell_shapes.font(run.font);
                let plan_cache = &mut shared.plan_cache;
                while offset < run.bytes.end {
                    let Some(cell) = cells.next() else { break };
                    let symbol = cell.symbol();
                    if symbol.is_empty() {
                        continue;
                    }
                    let shaped = shapes.get_or_shape(symbol, || {
                        shape_text(run.font, symbol, &mut self.buffer, plan_cache)
                    });
                    glyphs.extend(shaped.iter().map(|glyph| RowGlyph {
                        glyph_id: glyph.glyph_id,
                        cluster: offset as u32 + glyph.cluster,
                        x_advance: glyph.x_advance,
                        x_offset: glyph.x_offset,
                        run: run_idx as u16,
                    }));
                    offset += symbol.len();
                }
            }
        }

        if !per_cell {
            for (run_idx, run) in runs.iter().enumerate() {
                let text = &self.row[run.bytes.clone()];
                let shaped = shape_text(run.font, text, &mut self.buffer, &mut shared.plan_cache);
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

            let cover = self.covered[cell_idx];
            #[cfg(feature = "emoji")]
            if run.color
                && draw_color_glyph(
                    &SymbolRow {
                        fonts: &self.fonts,
                        cells: row_cells,
                        y,
                        colors: (self.reset_fg, self.reset_bg, self.transparent_reset_bg),
                    },
                    glyph,
                    run.font,
                    cell_idx,
                    &mut self.covered,
                    shared,
                    out,
                )
            {
                continue;
            }
            if cover != Cover::Free {
                if cover == Cover::SymbolTail {
                    continue;
                }
            } else if run.symbols
                && draw_symbol(
                    &SymbolRow {
                        fonts: &self.fonts,
                        cells: row_cells,
                        y,
                        colors: (self.reset_fg, self.reset_bg, self.transparent_reset_bg),
                    },
                    glyph,
                    run.font,
                    cell_idx,
                    &mut self.covered,
                    shared,
                    out,
                )
            {
                continue;
            }
            let cell = &row_cells[cell_idx];

            let (cell_font, cell_fake_bold, cell_fake_italic) =
                (run.font, run.fake_bold, run.fake_italic);

            use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
            let symbol = cell.symbol();
            let ch = symbol.chars().next().unwrap_or(' ');
            let ch_width = if symbol.len() == ch.len_utf8() {
                ch.width().unwrap_or(1).max(1)
            } else {
                symbol.width().clamp(1, 2)
            } as u32;
            let glyph_width_px = ch_width * cell_width_px;

            use crate::backend::programmatic_glyphs::is_programmatic_glyph;
            let is_programmatic = is_programmatic_glyph(ch);

            let style = key_style(cell);

            let key = if is_programmatic {
                Key {
                    style,
                    glyph: ch as u32,
                    font: last_resort_id,
                    cells: ch_width as u8,
                }
            } else {
                Key {
                    style,
                    glyph: glyph.glyph_id,
                    font: cell_font.id(),
                    cells: ch_width as u8,
                }
            };

            let cached = shared.cached.get(&key, glyph_width_px, cell_height_px);

            if !cached.cached() {
                if is_programmatic {
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

                    let bearing_offset_x = glyph.x_offset as f32 * advance_scale * fit_x;

                    let mut info = rustybuzz::GlyphInfo::default();
                    info.glyph_id = glyph.glyph_id;

                    let (rect, image) = rasterize_glyph(
                        cached,
                        metrics,
                        &info,
                        cell_fake_italic,
                        cell_fake_bold,
                        advance_scale,
                        Placement::Natural {
                            bearing_offset_x,
                            fit_x,
                        },
                    );

                    shared
                        .pending_cache_updates
                        .push((rect, glyph_coverage(&image)));
                }
            }

            let (fg, bg) = cell_colors(
                cell,
                self.reset_fg,
                self.reset_bg,
                self.transparent_reset_bg,
            );

            let instance = CellInstance {
                rect: [
                    px_u16(cell_idx as u32 * cell_width_px),
                    px_u16(y as u32 * cell_height_px),
                    px_u16(glyph_width_px),
                    px_u16(cell_height_px),
                ],
                uv: [px_u16(cached.x), px_u16(cached.y)],
                fg,
                bg,
            };
            if cover == Cover::Free {
                self.covered[cell_idx] = Cover::Glyph;
                out.cells.push(instance);
            } else {
                out.overlays.push(instance);
            }
        }

        self.glyphs = glyphs;
    }
}

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

        self.rows_dirty_since_take.clear();
        self.full_redraw_needed = true;

        Ok(())
    }

    fn size(&self) -> std::io::Result<ratatui::layout::Size> {
        let (inset_width, inset_height) = match self.viewport {
            Viewport::Full => (0, 0),
            Viewport::Shrink { width, height } => (width, height),
        };

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
    /// for partial redraw).
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

        let fonts = Arc::clone(&self.fonts);
        fonts.with_shared_cpu_state(|shared| {
            shared.cached.begin_flush();
            #[cfg(feature = "emoji")]
            shared.color.begin_flush();
            let generation_before_flush = shared.atlas_generation();

            for (y, &dirty) in was_dirty.iter().enumerate() {
                if !dirty {
                    continue;
                }
                self.reshape_row(y, width, shared);
                self.dirty_rows[y] = false;
            }

            let atlas_evicted_this_flush = shared.atlas_generation() != generation_before_flush;

            for (y, &dirty) in was_dirty.iter().enumerate() {
                if dirty {
                    continue;
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
        self.clear()
    }
}

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
            .with_transparent_reset_bg(false)
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
        let mut backend = TerminalBuilder::new(test_fonts())
            .with_dimensions(1, 1)
            .with_transparent_reset_bg(true)
            .build();

        let mut cell = Cell::default();
        cell.set_symbol("a");
        cell.modifier.insert(ratatui::style::Modifier::REVERSED);
        cell.fg = Color::Reset;
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

    #[test]
    fn flush_skips_vertex_regeneration_on_byte_identical_redraw() {
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
            !payload.cells().is_empty(),
            "first draw onto an empty buffer must generate instances"
        );

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

        let row_c_changed = cell_with_symbol("Z");
        RatatuiBackend::draw(&mut backend, [(0u16, 2u16, &row_c_changed)].into_iter())
            .expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");

        let incremental_rows: Vec<RowGeometry> = backend.row_geometry.clone();
        let _ = backend.take_draw_payload();

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

    #[test]
    fn a_cluster_cell_is_as_wide_as_the_whole_cluster() {
        let cell_width = test_fonts().min_width_px() as u16;
        for symbol in [
            "\u{2764}\u{FE0F}",
            "\u{1F1EF}\u{1F1F5}",
            "1\u{FE0F}\u{20E3}",
        ] {
            let mut backend = TerminalBuilder::new(test_fonts())
                .with_dimensions(2, 1)
                .build();
            let mut cell = Cell::default();
            cell.set_symbol(symbol);
            RatatuiBackend::draw(&mut backend, [(0u16, 0u16, &cell)].into_iter())
                .expect("draw failed");
            RatatuiBackend::flush(&mut backend).expect("flush failed");
            let payload = backend.take_draw_payload();
            assert_eq!(payload.cells().len(), 1, "{symbol:?}: one instance");
            assert_eq!(
                payload.cells()[0].rect[2],
                2 * cell_width,
                "{symbol:?} must span two cells"
            );
        }
    }

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
        let _ = backend.take_draw_payload();

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

        let clear = payload.cells()[0];
        assert_eq!(
            clear.rect,
            [0, cell_height_px, cell_width_px, cell_height_px]
        );
        assert_eq!(clear.bg, backend.initial_fill);
        assert_eq!(clear.fg[3], 0, "a row-clear instance must not draw a glyph");

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
            .with_initial_fill([0, 0, 0, 0])
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
        let _ = backend.take_draw_payload();

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

#[cfg(test)]
mod shaping_mode_tests {
    use super::*;
    use crate::fonts::{Font, Shaping};
    use ratatui::style::{Modifier, Style};

    const MPLUS: &[u8] = include_bytes!("../../examples/assets/fonts/Mplus1Code-Regular.ttf");
    const FUSION: &[u8] =
        include_bytes!("../../examples/assets/fonts/fusion-pixel-10px-monospaced-ja.ttf");
    const ATLAS: usize = crate::backend::CACHE_WIDTH as usize;

    fn coverage(shaping: Shaping) -> Vec<u8> {
        let mut fonts = Fonts::new(Font::new(FUSION).expect("fusion-pixel parses"), 16);
        fonts.add_regular_fonts([Font::new(MPLUS).expect("M+ parses")]);
        #[cfg(feature = "bold_italic_fonts")]
        fonts.add_bold_fonts([Font::new(MPLUS).expect("M+ parses")]);
        fonts.set_shaping(shaping);

        let backend = TerminalBuilder::new(Arc::new(fonts))
            .with_dimensions(40, 7)
            .build();
        let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
        let plain = Style::default();
        let rows = [
            ("The quick brown fox -> != == 0123456789", plain),
            ("日本語テキスト ｶﾅ 한글 mixed", plain),
            ("e\u{301} a\u{308} ┌─┐│█▓ ⣿ x", plain),
            ("Bold 日本 -> quick", plain.add_modifier(Modifier::BOLD)),
            ("Italic 한 != quick", plain.add_modifier(Modifier::ITALIC)),
            (
                "Both == quick",
                plain.add_modifier(Modifier::BOLD | Modifier::ITALIC),
            ),
            ("\u{E000} src \u{E001}x → y \u{E002}\u{E003} ", plain),
        ];
        terminal
            .draw(|frame| {
                for (y, (text, style)) in rows.iter().enumerate() {
                    frame.buffer_mut().set_string(0, y as u16, text, *style);
                }
            })
            .expect("draw");

        let payload = terminal.backend_mut().take_draw_payload();
        let mut atlas = vec![0u8; ATLAS * ATLAS];
        for (rect, bytes) in terminal.backend_mut().take_shared_glyph_uploads() {
            let width = rect.width as usize;
            for (row, line) in bytes.chunks_exact(width).enumerate() {
                let start = (rect.y as usize + row) * ATLAS + rect.x as usize;
                atlas[start..start + width].copy_from_slice(line);
            }
        }
        let width = payload.screen_width_px as usize;
        let mut image = vec![0u8; width * payload.screen_height_px as usize];
        for instance in &payload.instances {
            let [x, y, w, h] = instance.rect.map(usize::from);
            let [u, v] = instance.uv.map(usize::from);
            for dy in 0..h {
                for dx in 0..w {
                    let pixel = &mut image[(y + dy) * width + x + dx];
                    *pixel = (*pixel).max(atlas[(v + dy) * ATLAS + u + dx]);
                }
            }
        }
        image
    }

    #[test]
    fn cell_and_row_shaping_draw_the_same_pixels() {
        let row = coverage(Shaping::Row);
        assert!(row.iter().any(|&c| c > 0), "something must be drawn");
        assert!(row == coverage(Shaping::Cell));
    }
}

#[cfg(test)]
mod symbol_tests {
    use super::*;
    use crate::fonts::Font;
    use ratatui::style::{Color, Style};

    const MPLUS: &[u8] = include_bytes!("../../examples/assets/fonts/Mplus1Code-Regular.ttf");
    const FUSION: &[u8] =
        include_bytes!("../../examples/assets/fonts/fusion-pixel-10px-monospaced-ja.ttf");
    const ATLAS: usize = crate::backend::CACHE_WIDTH as usize;
    const ICON: &str = "\u{E000}";

    fn fonts(fit_symbols: bool) -> Fonts {
        let mut fonts = Fonts::new(Font::new(FUSION).expect("fusion-pixel parses"), 16);
        fonts.add_regular_fonts([Font::new(MPLUS).expect("M+ parses")]);
        fonts.set_fit_symbols(fit_symbols);
        fonts
    }

    fn render(
        fonts: Fonts,
        rows: &[&str],
        style: impl Fn(u16, u16) -> Style,
    ) -> (crate::backend::TerminalDrawPayload, Vec<u8>, usize) {
        let backend = TerminalBuilder::new(Arc::new(fonts))
            .with_dimensions(12, rows.len() as u16)
            .build();
        let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                let buffer = frame.buffer_mut();
                for (y, row) in rows.iter().enumerate() {
                    buffer.set_string(0, y as u16, row, Style::default());
                    for x in 0..12 {
                        buffer[(x, y as u16)].set_style(style(x, y as u16));
                    }
                }
            })
            .expect("draw");
        let payload = terminal.backend_mut().take_draw_payload();
        let mut atlas = vec![0u8; ATLAS * ATLAS];
        for (rect, bytes) in terminal.backend_mut().take_shared_glyph_uploads() {
            let width = rect.width as usize;
            for (row, line) in bytes.chunks_exact(width).enumerate() {
                let start = (rect.y as usize + row) * ATLAS + rect.x as usize;
                atlas[start..start + width].copy_from_slice(line);
            }
        }
        let width = payload.screen_width_px as usize;
        let mut image = vec![0u8; width * payload.screen_height_px as usize];
        for instance in &payload.instances {
            let [x, y, w, h] = instance.rect.map(usize::from);
            let [u, v] = instance.uv.map(usize::from);
            for dy in 0..h {
                for dx in 0..w {
                    let pixel = &mut image[(y + dy) * width + x + dx];
                    *pixel = (*pixel).max(atlas[(v + dy) * ATLAS + u + dx]);
                }
            }
        }
        (payload, image, width)
    }

    fn ink_bounds(
        image: &[u8],
        width: usize,
        row: usize,
        cell_height: usize,
        columns: usize,
    ) -> (usize, usize, usize, usize) {
        let mut bounds = (usize::MAX, 0, usize::MAX, 0);
        for y in row * cell_height..(row + 1) * cell_height {
            for x in 0..columns {
                if image[y * width + x] > 127 {
                    bounds = (
                        bounds.0.min(x),
                        bounds.1.max(x + 1),
                        bounds.2.min(y),
                        bounds.3.max(y + 1),
                    );
                }
            }
        }
        bounds
    }

    #[test]
    fn a_symbol_before_a_blank_spreads_over_both_cells() {
        let fonts = fonts(true);
        let (cw, ch) = (fonts.min_width_px() as u16, fonts.height_px() as u16);
        let (payload, image, width) = render(
            fonts,
            &[&format!("{ICON} a"), &format!("{ICON}\u{A0}a")],
            |_, _| Style::default(),
        );

        let row0: Vec<_> = payload.cells().iter().filter(|i| i.rect[1] == 0).collect();
        assert_eq!(row0[0].rect, [0, 0, 2 * cw, ch]);
        assert!(
            row0.iter().all(|i| i.rect[0] != cw),
            "the blank draws no glyph of its own"
        );
        let row1: Vec<_> = payload.cells().iter().filter(|i| i.rect[1] == ch).collect();
        assert_eq!(row1[0].rect, [0, ch, cw, ch]);
        assert_ne!(
            row0[0].uv, row1[0].uv,
            "one- and two-cell renderings are separate atlas entries"
        );

        let (x0, x1, y0, y1) = ink_bounds(&image, width, 0, ch as usize, 2 * cw as usize);
        assert!(
            x0 < 2 && x1 > (2 * cw - 2) as usize && x1 <= (2 * cw) as usize,
            "{x0}..{x1}"
        );
        assert!(
            (x1 - x0).abs_diff(y1 - y0) <= 2,
            "aspect kept: {}x{}",
            x1 - x0,
            y1 - y0
        );
        let (x0, x1, y0, y1) = ink_bounds(&image, width, 1, ch as usize, 2 * cw as usize);
        assert!(x1 <= cw as usize, "stays in its cell: {x0}..{x1}");
        assert!(
            (x1 - x0).abs_diff(y1 - y0) <= 2,
            "aspect kept: {}x{}",
            x1 - x0,
            y1 - y0
        );
    }

    #[test]
    fn the_blank_keeps_its_own_background() {
        let fonts = fonts(true);
        let cw = fonts.min_width_px() as u16;
        let (payload, ..) = render(fonts, &[&format!("{ICON} a")], |x, _| {
            Style::default().bg(if x == 1 {
                Color::Rgb(1, 2, 3)
            } else {
                Color::Rgb(9, 9, 9)
            })
        });
        let cells = payload.cells();
        assert_eq!(cells[0].rect[0..3], [0, 0, cw]);
        assert_eq!(cells[1].rect[0..3], [cw, 0, cw]);
        assert_eq!(
            cells[1].uv,
            [cells[0].uv[0] + cw, cells[0].uv[1]],
            "the slot's right half"
        );
        assert_eq!(cells[1].fg, cells[0].fg, "drawn in the symbol's color");
        assert_eq!(cells[1].bg, [1, 2, 3, 255]);
        assert_eq!(cells[0].bg, [9, 9, 9, 255]);
        assert!(payload.overlays().is_empty());
    }

    #[test]
    fn symbols_stay_in_one_cell_in_a_run_at_the_row_end_or_when_disabled() {
        let fonts_on = fonts(true);
        let (cw, ch) = (fonts_on.min_width_px() as u16, fonts_on.height_px() as u16);
        let widths = |payload: &crate::backend::TerminalDrawPayload, row: u16| -> Vec<u16> {
            payload
                .cells()
                .iter()
                .filter(|i| i.rect[1] == row * ch)
                .map(|i| i.rect[2])
                .collect()
        };
        let row_end = format!("{}{ICON}", " ".repeat(11));
        let (payload, ..) = render(
            fonts_on,
            &[&format!("{ICON}{ICON} "), &row_end, "\u{E0B0}\u{E001} x"],
            |_, _| Style::default(),
        );
        assert_eq!(widths(&payload, 0)[..2], [cw, cw]);
        assert_eq!(*widths(&payload, 1).last().unwrap(), cw, "last column");
        assert_eq!(widths(&payload, 2)[..2], [cw, 2 * cw]);

        let (payload, image, width) = render(fonts(false), &[&format!("{ICON} a")], |_, _| {
            Style::default()
        });
        assert_eq!(
            widths(&payload, 0)[..2],
            [cw, cw],
            "disabled: drawn like text"
        );
        let (x0, x1, ..) = ink_bounds(&image, width, 0, ch as usize, 2 * cw as usize);
        assert!(
            x1 <= cw as usize && x1 - x0 >= cw as usize - 1,
            "squeezed into one cell: {x0}..{x1}"
        );
    }

    #[test]
    fn a_wide_drawn_ambiguous_symbol_spreads_like_an_icon() {
        let fonts = fonts(true);
        let (cw, ch) = (fonts.min_width_px() as u16, fonts.height_px() as u16);
        let (payload, ..) = render(fonts, &["→ x", "→x"], |_, _| Style::default());
        assert_eq!(payload.cells()[0].rect, [0, 0, 2 * cw, ch]);
        let row1: Vec<_> = payload.cells().iter().filter(|i| i.rect[1] == ch).collect();
        assert_eq!(row1[0].rect, [0, ch, cw, ch]);
    }
}

#[cfg(all(test, feature = "emoji"))]
mod emoji_tests {
    use super::*;
    use crate::fonts::Font;
    use ratatui::backend::Backend as RatatuiBackend;
    use ratatui::style::Color;

    const MPLUS: &[u8] = include_bytes!("../../examples/assets/fonts/Mplus1Code-Regular.ttf");
    const COLRV1: &[u8] = include_bytes!("testdata/noto_colrv1_subset.ttf");
    const CBDT: &[u8] = include_bytes!("testdata/noto_cbdt_subset.ttf");
    const FIXTURES: [&[u8]; 2] = [COLRV1, CBDT];

    fn fonts(emoji: &'static [u8]) -> Arc<Fonts> {
        let mut fonts = Fonts::new(Font::new(MPLUS).expect("M+ parses"), 16);
        fonts.add_emoji_fonts([Font::new(emoji).expect("emoji fixture parses")]);
        Arc::new(fonts)
    }

    fn backend_with(fonts: Arc<Fonts>, cols: u16, cells: &[(u16, &str)]) -> BevyTerminalBackend {
        let mut backend = TerminalBuilder::new(fonts).with_dimensions(cols, 1).build();
        let cells: Vec<(u16, u16, Cell)> = cells
            .iter()
            .map(|&(x, symbol)| {
                let mut cell = Cell::default();
                cell.set_symbol(symbol);
                (x, 0, cell)
            })
            .collect();
        RatatuiBackend::draw(
            &mut backend,
            cells.iter().map(|(x, y, cell)| (*x, *y, cell)),
        )
        .expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");
        backend
    }

    #[test]
    fn a_color_glyph_is_one_two_cell_instance_between_text_cells() {
        for emoji in FIXTURES {
            let fonts = fonts(emoji);
            let cw = fonts.min_width_px() as u16;
            let mut backend = backend_with(fonts, 4, &[(0, "a"), (1, "\u{1F600}"), (3, "b")]);
            let payload = backend.take_draw_payload();
            assert_eq!(payload.cells().len(), 2, "a and b");
            assert_eq!(payload.colors().len(), 1);
            assert!(payload.overlays().is_empty());
            let color = payload.colors()[0];
            assert_eq!(color.rect[0..3], [cw, 0, 2 * cw]);
            assert_eq!(color.fg, [0; 4], "the texel is the glyph, untinted");
        }
    }

    #[test]
    fn a_combining_mark_on_an_emoji_is_an_overlay_from_the_text_font() {
        for emoji in FIXTURES {
            let fonts = fonts(emoji);
            let cw = fonts.min_width_px() as u16;
            let mut backend = backend_with(fonts, 2, &[(0, "\u{1F600}\u{301}")]);
            let payload = backend.take_draw_payload();
            assert!(payload.cells().is_empty());
            assert_eq!(payload.colors().len(), 1);
            assert_eq!(payload.overlays().len(), 1);
            assert_eq!(payload.overlays()[0].rect[0..3], [cw / 2, 0, cw]);
            assert_eq!(
                payload.overlays()[0].fg,
                [255; 4],
                "the mark is tinted with the cell's text color"
            );
        }
    }

    #[test]
    fn overlay_marks_exclude_what_belongs_to_the_emoji_sequence() {
        for mark in ['\u{301}', '\u{3099}', '\u{20DD}'] {
            assert!(is_overlay_mark(mark), "{mark:?}");
        }
        for other in ['\u{20E3}', '\u{FE0F}', '\u{200D}', '\u{1F3FD}', 'a'] {
            assert!(!is_overlay_mark(other), "{other:?}");
        }
    }

    #[test]
    fn selector_and_keycap_sequences_have_no_overlays() {
        for emoji in FIXTURES {
            for symbol in ["\u{2764}\u{FE0F}", "1\u{FE0F}\u{20E3}"] {
                let mut backend = backend_with(fonts(emoji), 2, &[(0, symbol)]);
                let payload = backend.take_draw_payload();
                assert!(payload.overlays().is_empty(), "{symbol:?}");
            }
        }
    }

    #[test]
    fn a_sequence_is_one_color_instance_over_its_two_cells() {
        for emoji in FIXTURES {
            for symbol in [
                "\u{2764}\u{FE0F}",
                "\u{1F44D}\u{1F3FD}",
                "\u{1F1EF}\u{1F1F5}",
                "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}",
            ] {
                let fonts = fonts(emoji);
                let cw = fonts.min_width_px() as u16;
                let mut backend = backend_with(fonts, 2, &[(0, symbol)]);
                let payload = backend.take_draw_payload();
                assert!(payload.cells().is_empty(), "{symbol:?}");
                assert_eq!(payload.colors().len(), 1, "{symbol:?}");
                assert_eq!(payload.colors()[0].rect[2], 2 * cw, "{symbol:?}");
                assert!(payload.overlays().is_empty(), "{symbol:?}");
            }
        }
    }

    #[test]
    fn a_glyph_without_color_data_is_not_handled() {
        let fonts = fonts(COLRV1);
        let font = Font::new(MPLUS).expect("M+ parses");
        let glyph = RowGlyph {
            glyph_id: font.font().glyph_index('a').expect("M+ has a").0 as u32,
            cluster: 0,
            x_advance: 0,
            x_offset: 0,
            run: 0,
        };
        let cells = [{
            let mut cell = Cell::default();
            cell.set_symbol("a");
            cell
        }];
        let mut covered = [Cover::Free];
        let mut out = RowGeometry::default();
        let handled = fonts.with_shared_cpu_state(|shared| {
            let handled = draw_color_glyph(
                &SymbolRow {
                    fonts: &fonts,
                    cells: &cells,
                    y: 0,
                    colors: ([255; 3], [0; 3], false),
                },
                &glyph,
                &font,
                0,
                &mut covered,
                shared,
                &mut out,
            );
            assert!(shared.pending_color_updates.is_empty());
            assert!(shared.pending_cache_updates.is_empty());
            handled
        });
        assert!(!handled);
        assert!(out.colors.is_empty() && out.cells.is_empty() && out.overlays.is_empty());
        assert_eq!(covered, [Cover::Free]);
    }

    #[test]
    fn both_formats_rasterize_to_premultiplied_color() {
        for emoji in FIXTURES {
            let fonts = fonts(emoji);
            let (cw, ch) = (fonts.min_width_px(), fonts.height_px());
            let backend = backend_with(fonts, 2, &[(0, "\u{1F600}")]);
            let (size, uploads) = backend.take_shared_color_uploads();
            assert_eq!(uploads.len(), 1);
            let (rect, image) = &uploads[0];
            assert_eq!((rect.width, rect.height), (2 * cw, ch));
            assert!(rect.x + rect.width <= size.0 && rect.y + rect.height <= size.1);
            assert_eq!(image.len(), (rect.width * rect.height * 4) as usize);
            let pixels = image.as_chunks::<4>().0;
            assert!(
                pixels.iter().any(|px| px[3] > 0),
                "something is drawn (alpha)"
            );
            assert!(
                pixels.iter().any(|px| px[0].max(px[1]).max(px[2]) > 100),
                "and it has color (RGB), not only alpha"
            );
            assert!(
                pixels.iter().all(|px| px[..3].iter().all(|&c| c <= px[3])),
                "premultiplied: no channel above alpha"
            );
        }
    }

    #[test]
    fn unchanged_content_queues_no_further_uploads() {
        for emoji in FIXTURES {
            let mut backend = backend_with(
                fonts(emoji),
                4,
                &[(0, "a"), (1, "\u{1F600}\u{301}"), (3, "b")],
            );
            let (_, first) = backend.take_shared_color_uploads();
            assert!(!first.is_empty());
            assert_eq!(backend.take_shared_glyph_uploads().len(), 3, "a, b, mark");
            let _ = backend.take_draw_payload();

            backend
                .dirty_rows
                .iter_mut()
                .for_each(|dirty| *dirty = true);
            backend.cells_changed_last_draw = true;
            RatatuiBackend::flush(&mut backend).expect("flush failed");
            assert!(backend.take_shared_color_uploads().1.is_empty());
            assert!(backend.take_shared_glyph_uploads().is_empty());
        }
    }

    #[test]
    fn a_partial_payload_orders_clear_then_cells_then_colors() {
        let mut backend = TerminalBuilder::new(fonts(COLRV1))
            .with_dimensions(3, 2)
            .build();
        let draw = |backend: &mut BevyTerminalBackend, cells: Vec<(u16, u16, &str)>| {
            let cells: Vec<(u16, u16, Cell)> = cells
                .into_iter()
                .map(|(x, y, symbol)| {
                    let mut cell = Cell::default();
                    cell.set_symbol(symbol);
                    cell.bg = Color::Rgb(1, 2, 3);
                    (x, y, cell)
                })
                .collect();
            RatatuiBackend::draw(backend, cells.iter().map(|(x, y, c)| (*x, *y, c)))
                .expect("draw failed");
            RatatuiBackend::flush(backend).expect("flush failed");
        };
        draw(
            &mut backend,
            vec![(0, 0, "a"), (0, 1, "b"), (1, 1, "\u{1F600}")],
        );
        let _ = backend.take_draw_payload();

        draw(&mut backend, vec![(0, 1, "x"), (1, 1, "\u{1F680}")]);
        let payload = backend.take_draw_payload();
        assert!(!payload.is_full());
        let ch = backend.fonts.height_px() as u16;
        assert_eq!(payload.cells().len(), 2, "row clear + x");
        assert_eq!(payload.cells()[0].fg[3], 0, "the clear comes first");
        assert_eq!(payload.cells()[0].rect[1], ch);
        assert_eq!(payload.cells()[1].rect[0], 0, "then the text cell");
        assert_eq!(payload.colors().len(), 1, "then the color glyph");
        assert!(payload.overlays().is_empty());
        let all: Vec<_> = payload.cells().iter().chain(payload.colors()).collect();
        assert_eq!(all.len(), payload.instances.len());
    }
}
