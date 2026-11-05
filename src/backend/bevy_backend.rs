use std::sync::Arc;

use crate::backend::CellInstance;
use crate::backend::Viewport;
use crate::backend::glyph_constraint::{CellClass, GlyphConstraint};
use crate::backend::rasterize::{Placement, rasterize_glyph};
use crate::colors::{Rgb, TerminalPalette};
use crate::fonts::Fonts;
use crate::utils::text_atlas::Key;
use ratatui::buffer::Cell;
use ratatui::style::Modifier;
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
    /// The blink modifiers present in this row (as shaped).
    #[cfg(feature = "blink")]
    blink: Modifier,
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
    /// Whether any of its cells has no glyph in any font but a procedural
    /// fallback - false for practically every run, like `symbols`.
    fallbacks: bool,
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
                run.fallbacks |= class.fallback;
            }
            _ => runs.push(FontRun {
                font,
                fake_bold,
                fake_italic,
                symbols: class.is_symbol(),
                fallbacks: class.fallback,
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
    let glyph_buffer = match plan_cache.get(font, &mut unicode) {
        Some(plan) => rustybuzz::shape_with_plan(font.font(), plan, unicode),
        None => rustybuzz::shape(font.font(), &[], unicode),
    };
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
    /// Scratch for `shape_row`: the row with invisible cells blanked.
    visible_row: Vec<Cell>,
    decorations: crate::fonts::DecorationMetrics,

    /// Per-row geometry cache, indexed by row `y`, letting `flush()` skip
    /// reshaping+re-rasterizing rows `dirty_rows` says are unchanged.
    row_geometry: Vec<RowGeometry>,

    pub(super) palette: CellPalette,
    cursor_visible: bool,
    cursor_style: CursorStyle,
    /// Which blink classes are currently in their "off" half.
    #[cfg(feature = "blink")]
    blink_off: Modifier,
    /// Color shown before any content has been drawn (or while nothing is
    /// drawn at all - e.g. mid-resize): the render world's `LoadOp::Clear`
    /// color whenever a draw payload's vertex data is empty.
    pub(super) initial_fill: [u8; 4],
}

/// Everything `cell_colors` needs besides the cell.
pub(super) struct CellPalette {
    pub(super) reset_fg: Rgb,
    pub(super) reset_bg: Rgb,
    /// If true, a cell whose *effective* background is `Color::Reset`
    /// (accounting for `Modifier::REVERSED`) gets background alpha 0 instead
    /// of 255, making it show through to whatever is behind the terminal's
    /// surface (a world-quad with `AlphaMode::Blend`, or a transparent
    /// background UI node).
    pub(super) transparent_reset_bg: bool,
    pub(super) dim_alpha: u8,
    pub(super) colors: TerminalPalette,
}

impl CellPalette {
    #[inline(always)]
    fn resolve(&self, color: ratatui::style::Color, reset: Rgb) -> Rgb {
        self.colors.resolve(color, reset)
    }
}

/// How the backend draws the cursor ratatui shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorStyle {
    /// The cell is drawn reversed.
    #[default]
    Block,
    /// An outline around the cell, for an unfocused terminal.
    HollowBlock,
    /// A bar under the cell.
    Underline,
    /// A thin vertical bar at the left edge of the cell.
    Bar,
    /// The cursor is not drawn.
    Hidden,
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
    dim_alpha: u8,
    palette: TerminalPalette,
    cursor_style: CursorStyle,
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
            dim_alpha: DEFAULT_DIM_ALPHA,
            palette: TerminalPalette::default(),
            cursor_style: CursorStyle::default(),
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

    /// Foreground alpha of `Modifier::DIM` cells (default 128).
    pub fn with_dim_alpha(mut self, alpha: u8) -> Self {
        self.dim_alpha = alpha;
        self
    }

    /// The RGB values of ratatui's named and indexed colors.
    pub fn with_palette(mut self, palette: TerminalPalette) -> Self {
        self.palette = palette;
        self
    }

    /// How the cursor ratatui shows is drawn (default [`CursorStyle::Block`]).
    pub fn with_cursor_style(mut self, style: CursorStyle) -> Self {
        self.cursor_style = style;
        self
    }

    /// Color shown before any content has been drawn.
    pub fn with_initial_fill(mut self, initial_fill: [u8; 4]) -> Self {
        self.initial_fill = initial_fill;
        self
    }

    /// Build the BevyTerminalBackend.
    pub fn build(self) -> BevyTerminalBackend {
        let decorations = self.fonts.decoration_metrics();
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
            visible_row: vec![],
            decorations,
            row_geometry: vec![],
            palette: CellPalette {
                reset_fg: self.reset_fg,
                reset_bg: self.reset_bg,
                transparent_reset_bg: self.transparent_reset_bg,
                dim_alpha: self.dim_alpha,
                colors: self.palette,
            },
            cursor_visible: false,
            cursor_style: self.cursor_style,
            #[cfg(feature = "blink")]
            blink_off: Modifier::empty(),
            initial_fill: self.initial_fill,
        }
    }
}

/// Default foreground alpha of `Modifier::DIM` cells.
pub(crate) const DEFAULT_DIM_ALPHA: u8 = 128;

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
fn cell_colors(cell: &Cell, palette: &CellPalette) -> ([u8; 4], [u8; 4]) {
    let reverse = cell.modifier.contains(Modifier::REVERSED);
    let (fg, bg) = if reverse {
        (
            palette.resolve(cell.bg, palette.reset_bg),
            palette.resolve(cell.fg, palette.reset_fg),
        )
    } else {
        (
            palette.resolve(cell.fg, palette.reset_fg),
            palette.resolve(cell.bg, palette.reset_bg),
        )
    };
    let bg_source = if reverse { cell.fg } else { cell.bg };
    let bg_alpha =
        if palette.transparent_reset_bg && matches!(bg_source, ratatui::style::Color::Reset) {
            0
        } else {
            255
        };
    let fg_alpha = if cell.modifier.contains(Modifier::DIM) {
        palette.dim_alpha
    } else {
        255
    };
    (
        [fg[0], fg[1], fg[2], fg_alpha],
        [bg[0], bg[1], bg[2], bg_alpha],
    )
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
    palette: &'a CellPalette,
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
    let Some(cell) = row.cells.get(x) else {
        return false;
    };
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
                && covered.get(x + 1) == Some(&Cover::Free);
            let prev = x
                .checked_sub(1)
                .and_then(|prev| row.cells.get(prev))
                .map(&mut class_of);
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

    let (fg, bg) = cell_colors(cell, row.palette);
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
    set_cover(covered, x, Cover::Glyph);
    if !spreads {
        out.cells.push(instance(x_px, width_px, cached.x, bg));
        return true;
    }
    let tail_bg = row
        .cells
        .get(x + 1)
        .map_or(bg, |tail| cell_colors(tail, row.palette).1);
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
    set_cover(covered, x + 1, Cover::SymbolTail);
    true
}

/// Marks cell `x` of the row being shaped, if it exists.
fn set_cover(covered: &mut [Cover], x: usize, cover: Cover) {
    if let Some(slot) = covered.get_mut(x) {
        *slot = cover;
    }
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

    match covered.get(x) {
        Some(Cover::Color) => return true,
        Some(Cover::Free) => {}
        _ => return false,
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

    let Some(cell) = row.cells.get(x) else {
        return false;
    };
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

    let (fg, bg) = cell_colors(cell, row.palette);
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
    set_cover(covered, x, Cover::Color);

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
                px_u16(x_px + cells.saturating_sub(1) * cell_width_px / 2),
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
            && self
                .rows_dirty_since_take
                .get(..height)
                .is_some_and(|rows| rows.iter().all(|&d| d));
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
        let Some(slot) = self.row_geometry.get_mut(y) else {
            return;
        };
        let mut geometry = std::mem::take(slot);
        self.shape_row(y, width, shared, &mut geometry);
        geometry.atlas_generation = shared.atlas_generation();
        if let Some(slot) = self.row_geometry.get_mut(y) {
            *slot = geometry;
        }
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
        self.decorations = new_fonts.decoration_metrics();
        self.fonts = new_fonts;
    }

    pub(crate) fn dim_alpha(&self) -> u8 {
        self.palette.dim_alpha
    }

    /// Sets the foreground alpha of `Modifier::DIM` cells, restyling every row if it changed.
    pub fn set_dim_alpha(&mut self, alpha: u8) {
        if self.palette.dim_alpha == alpha {
            return;
        }
        self.palette.dim_alpha = alpha;
        self.restyle_all_rows();
    }

    pub(crate) fn palette(&self) -> &TerminalPalette {
        &self.palette.colors
    }

    /// Replaces the color palette, restyling every row if it changed.
    pub fn set_palette(&mut self, palette: TerminalPalette) {
        if self.palette.colors == palette {
            return;
        }
        self.palette.colors = palette;
        self.restyle_all_rows();
    }

    pub(crate) fn cursor_style(&self) -> CursorStyle {
        self.cursor_style
    }

    /// Sets how the cursor is drawn, reshaping its row if it is shown and the style changed.
    pub fn set_cursor_style(&mut self, style: CursorStyle) {
        if self.cursor_style == style {
            return;
        }
        self.cursor_style = style;
        if self.touch_cursor_row(self.cursor.1) {
            self.reshape_dirty_rows();
        }
    }

    /// Marks row `y` dirty if the cursor is shown and that row exists, returning whether it did.
    fn touch_cursor_row(&mut self, y: u16) -> bool {
        if !self.cursor_visible {
            return false;
        }
        let Some(dirty) = self.dirty_rows.get_mut(y as usize) else {
            return false;
        };
        *dirty = true;
        self.cells_changed_last_draw = true;
        true
    }

    /// Whether the cursor is drawn in row `y`, and at which column.
    fn cursor_column(&self, y: usize) -> Option<usize> {
        let shown = self.cursor_visible
            && self.cursor_style != CursorStyle::Hidden
            && self.cursor.1 as usize == y;
        #[cfg(feature = "blink")]
        let shown = shown && !self.blink_off.contains(Modifier::SLOW_BLINK);
        shown.then_some(self.cursor.0 as usize)
    }

    /// Reshapes the rows `dirty_rows` marks, then any clean row whose
    /// geometry predates an atlas eviction.
    fn reshape_dirty_rows(&mut self) {
        let Ok(bounds) = ratatui::backend::Backend::size(self) else {
            return;
        };
        let width = bounds.width as usize;
        // Callers other than `flush` (style setters, blink) can run before the
        // first draw has sized the per-row state to the grid.
        let height = (bounds.height as usize)
            .min(self.dirty_rows.len())
            .min(self.row_geometry.len());

        // Never clear `pending_cache_updates` here: it holds uploads for slots
        // the atlas already marks cached, queued by earlier flushes not yet
        // taken. Dropping them leaves those glyphs invisible for good.

        self.rows_dirty_since_take
            .resize(bounds.height as usize, false);

        // Pass 1 clears `dirty_rows` as it goes; pass 2 needs the rows that
        // were clean on entry, so snapshot them first.
        let was_dirty: Vec<bool> = self.dirty_rows.iter().take(height).copied().collect();
        for (since_take, &dirty) in self.rows_dirty_since_take.iter_mut().zip(&was_dirty) {
            *since_take |= dirty;
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
                if let Some(row_dirty) = self.dirty_rows.get_mut(y) {
                    *row_dirty = false;
                }
            }

            let atlas_evicted_this_flush = shared.atlas_generation() != generation_before_flush;

            for (y, &dirty) in was_dirty.iter().enumerate() {
                if dirty {
                    continue;
                }

                let cache_valid = !atlas_evicted_this_flush
                    && self
                        .row_geometry
                        .get(y)
                        .is_some_and(|row| row.atlas_generation == generation_before_flush);

                if cache_valid {
                    continue;
                }

                self.reshape_row(y, width, shared);
            }
        });
    }

    /// Reshapes every row, so the next payload is a full one; for a change of color settings.
    fn restyle_all_rows(&mut self) {
        let drawn = ratatui::backend::Backend::size(self)
            .is_ok_and(|bounds| self.dirty_rows.len() >= bounds.height as usize);
        if !drawn {
            return;
        }
        self.dirty_rows.fill(true);
        self.reshape_dirty_rows();
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
        let row_cells = self.cells.get(row_start..row_end).unwrap_or(&[]);
        let row_mods = row_cells
            .iter()
            .fold(Modifier::empty(), |mods, cell| mods | cell.modifier);
        #[cfg(feature = "blink")]
        {
            out.blink = row_mods & (Modifier::SLOW_BLINK | Modifier::RAPID_BLINK);
            if self.cursor_visible
                && self.cursor_style != CursorStyle::Hidden
                && self.cursor.1 as usize == y
            {
                out.blink |= Modifier::SLOW_BLINK;
            }
        }
        let invisible = self.invisible_modifiers();
        let cursor_x = self.cursor_column(y);
        let reversed_x = cursor_x.filter(|_| self.cursor_style == CursorStyle::Block);
        let mut visible_row = std::mem::take(&mut self.visible_row);
        let row_cells: &[Cell] = if row_mods.intersects(invisible) || reversed_x.is_some() {
            blank_invisible(row_cells, invisible, &mut visible_row);
            if let Some(cell) = reversed_x.and_then(|x| visible_row.get_mut(x)) {
                cell.modifier.toggle(Modifier::REVERSED);
            }
            &visible_row
        } else {
            row_cells
        };

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
            self.visible_row = visible_row;
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
                let text = self.row.get(run.bytes.clone()).unwrap_or("");
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
            let Some(run) = runs.get(glyph.run as usize) else {
                continue;
            };
            let Some(&cell_idx) = self.rowmap.get(glyph.cluster as usize) else {
                continue;
            };
            let cell_idx = cell_idx as usize;
            let (Some(cell), Some(&cover)) = (row_cells.get(cell_idx), self.covered.get(cell_idx))
            else {
                continue;
            };
            let fallback = run.fallbacks && glyph.glyph_id == 0 && {
                let ch = cell.symbol().chars().next().unwrap_or(' ');
                crate::backend::programmatic_glyphs::fallback_symbols::use_fallback(0, ch)
            };
            #[cfg(feature = "emoji")]
            if run.color
                && draw_color_glyph(
                    &SymbolRow {
                        fonts: &self.fonts,
                        cells: row_cells,
                        y,
                        palette: &self.palette,
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
                && !fallback
                && draw_symbol(
                    &SymbolRow {
                        fonts: &self.fonts,
                        cells: row_cells,
                        y,
                        palette: &self.palette,
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
            let is_programmatic = is_programmatic_glyph(ch) || fallback;

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
                    queue_programmatic_glyph(
                        ch,
                        *cached,
                        glyph_width_px,
                        cell_height_px,
                        &mut shared.pending_cache_updates,
                    );
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

            let (fg, bg) = cell_colors(cell, &self.palette);

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
                set_cover(&mut self.covered, cell_idx, Cover::Glyph);
                out.cells.push(instance);
            } else {
                out.overlays.push(instance);
            }
        }

        self.glyphs = glyphs;

        if row_mods.intersects(Modifier::UNDERLINED | Modifier::CROSSED_OUT) {
            self.push_decorations(row_cells, y, shared, out);
        }
        if let Some(x) = cursor_x {
            self.push_cursor(row_cells, x, y, shared, out);
        }
        self.visible_row = visible_row;
    }

    /// The modifiers that blank a cell.
    fn invisible_modifiers(&self) -> Modifier {
        #[cfg(feature = "blink")]
        return Modifier::HIDDEN | self.blink_off;
        #[cfg(not(feature = "blink"))]
        Modifier::HIDDEN
    }

    /// Whether switching to the `off` blink phase changes any row.
    #[cfg(feature = "blink")]
    pub(crate) fn blink_needs_update(&self, off: Modifier) -> bool {
        let changed = self.blink_off ^ off;
        !changed.is_empty()
            && self
                .row_geometry
                .iter()
                .any(|row| row.blink.intersects(changed))
    }

    /// Switches to the `off` blink phase, reshaping only rows with a blink class that changed.
    #[cfg(feature = "blink")]
    pub(crate) fn set_blink_off(&mut self, off: Modifier) {
        let changed = self.blink_off ^ off;
        self.blink_off = off;
        for (dirty, row) in self.dirty_rows.iter_mut().zip(&self.row_geometry) {
            if row.blink.intersects(changed) {
                *dirty = true;
            }
        }
        self.reshape_dirty_rows();
    }

    /// Emits the overlay bars of a non-block cursor at column `x` of row `y`.
    fn push_cursor(
        &self,
        row_cells: &[Cell],
        x: usize,
        y: usize,
        shared: &mut crate::fonts::SharedFontCpuState,
        out: &mut RowGeometry,
    ) {
        let Some(cell) = row_cells.get(x) else {
            return;
        };
        let (cw, ch) = (self.fonts.min_width_px(), self.fonts.height_px());
        let light = crate::backend::programmatic_glyphs::box_lines::LineMetrics::new(cw, ch).light;
        let d = self.decorations;
        let thickness = d.underline_thickness.saturating_mul(2).min(ch);
        let (left, top) = (x as u32 * cw, y as u32 * ch);
        let bars: &[[u32; 4]] = match self.cursor_style {
            CursorStyle::Underline => &[[
                left,
                top + d.underline_top.min(ch.saturating_sub(thickness)),
                cw,
                thickness,
            ]],
            CursorStyle::Bar => &[[left, top, (cw / 8).max(1), ch]],
            CursorStyle::HollowBlock => &[
                [left, top, cw, light],
                [left, top + ch.saturating_sub(light), cw, light],
                [left, top, light, ch],
                [left + cw.saturating_sub(light), top, light, ch],
            ],
            CursorStyle::Block | CursorStyle::Hidden => return,
        };
        let solid = solid_slot(&self.fonts, shared);
        let fg = cell_colors(cell, &self.palette).0;
        for &[bx, by, bw, bh] in bars {
            out.overlays.push(CellInstance {
                rect: [px_u16(bx), px_u16(by), px_u16(bw), px_u16(bh)],
                uv: solid,
                fg,
                bg: [0; 4],
            });
        }
    }

    /// Emits one overlay bar per cell for each underlined or crossed-out cell of `row_cells`.
    fn push_decorations(
        &self,
        row_cells: &[Cell],
        y: usize,
        shared: &mut crate::fonts::SharedFontCpuState,
        out: &mut RowGeometry,
    ) {
        use unicode_width::UnicodeWidthStr;

        let cell_width_px = self.fonts.min_width_px();
        let cell_height_px = self.fonts.height_px();
        let solid = solid_slot(&self.fonts, shared);
        let d = self.decorations;
        let bars = [
            (Modifier::UNDERLINED, d.underline_top, d.underline_thickness),
            (
                Modifier::CROSSED_OUT,
                d.strikeout_top,
                d.strikeout_thickness,
            ),
        ];
        for (x, cell) in row_cells.iter().enumerate() {
            if !cell
                .modifier
                .intersects(Modifier::UNDERLINED | Modifier::CROSSED_OUT)
                || cell.symbol().is_empty()
            {
                continue;
            }
            let fg = cell_colors(cell, &self.palette).0;
            #[cfg(feature = "underline_color")]
            let underline = if cell.underline_color == ratatui::style::Color::Reset {
                fg
            } else {
                let [r, g, b] = self
                    .palette
                    .resolve(cell.underline_color, self.palette.reset_fg);
                [r, g, b, fg[3]]
            };
            #[cfg(not(feature = "underline_color"))]
            let underline = fg;
            let cells = if cell.symbol().len() == 1 {
                1
            } else {
                cell.symbol().width().clamp(1, 2)
            };
            for (modifier, top, thickness) in bars {
                if !cell.modifier.contains(modifier) {
                    continue;
                }
                let fg = if modifier == Modifier::UNDERLINED {
                    underline
                } else {
                    fg
                };
                for i in 0..cells {
                    out.overlays.push(CellInstance {
                        rect: [
                            px_u16((x + i) as u32 * cell_width_px),
                            px_u16(y as u32 * cell_height_px + top),
                            px_u16(cell_width_px),
                            px_u16(thickness),
                        ],
                        uv: solid,
                        fg,
                        bg: [0; 4],
                    });
                }
            }
        }
    }
}

/// Copies `cells` into `out`, every cell with an `invisible` modifier (and
/// the cells its wide symbol covers) as a blank with the same colors.
fn blank_invisible(cells: &[Cell], invisible: Modifier, out: &mut Vec<Cell>) {
    use unicode_width::UnicodeWidthStr;

    out.clear();
    out.extend_from_slice(cells);
    let mut x = 0;
    while let Some(cell) = out.get(x) {
        if !cell.modifier.intersects(invisible) {
            x += 1;
            continue;
        }
        let width = cell.symbol().width().max(1);
        let mut blank = cell.clone();
        blank.set_symbol(" ");
        blank
            .modifier
            .remove(Modifier::UNDERLINED | Modifier::CROSSED_OUT);
        for cell in out.iter_mut().skip(x).take(width) {
            *cell = blank.clone();
        }
        x += width;
    }
}

/// Rasterizes a procedural glyph into its atlas slot `rect` and queues the upload.
fn queue_programmatic_glyph(
    ch: char,
    rect: crate::utils::text_atlas::CacheRect,
    width: u32,
    height: u32,
    pending: &mut Vec<(crate::utils::text_atlas::CacheRect, Vec<u8>)>,
) {
    use crate::backend::programmatic_glyphs::render_programmatic_glyph;

    let pixmap = render_programmatic_glyph(ch, width, height).or_else(|| {
        crate::backend::programmatic_glyphs::fallback_symbols::render(ch, width, height)
    });
    if let Some(pixmap) = pixmap {
        pending.push((rect, pixmap_coverage(&pixmap)));
    } else {
        tracing::warn!(
            "Failed to render programmatic glyph '{}' (U+{:04X})",
            ch,
            ch as u32
        );
    }
}

/// Atlas origin of the procedurally drawn full block, uploading it if the
/// atlas lost it.
fn solid_slot(fonts: &Fonts, shared: &mut crate::fonts::SharedFontCpuState) -> [u16; 2] {
    let (width, height) = (fonts.min_width_px(), fonts.height_px());
    let key = Key {
        style: Modifier::empty(),
        glyph: '█' as u32,
        font: fonts.last_resort_id(),
        cells: 1,
    };
    let cached = shared.cached.get(&key, width, height);
    if !cached.cached() {
        queue_programmatic_glyph(
            '█',
            *cached,
            width,
            height,
            &mut shared.pending_cache_updates,
        );
    }
    [px_u16(cached.x), px_u16(cached.y)]
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

            let Some(slot) = self.cells.get_mut(index) else {
                continue;
            };
            *slot = cell.clone();

            let width = cell.symbol().width().max(1);
            let start = (index + 1).min(self.cells.len());
            let end = (index + width).min(self.cells.len());
            if let Some(tail) = self.cells.get_mut(start..end) {
                tail.fill(NULL_CELL);
            }
            if let Some(dirty) = self.dirty_rows.get_mut(y as usize) {
                *dirty = true;
            }
        }

        Ok(())
    }

    fn hide_cursor(&mut self) -> std::io::Result<()> {
        if self.cursor_visible {
            self.touch_cursor_row(self.cursor.1);
            self.cursor_visible = false;
        }
        Ok(())
    }

    fn show_cursor(&mut self) -> std::io::Result<()> {
        if !self.cursor_visible {
            self.cursor_visible = true;
            self.touch_cursor_row(self.cursor.1);
        }
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
        let cursor = (
            pos.x.min(bounds.width.saturating_sub(1)),
            pos.y.min(bounds.height.saturating_sub(1)),
        );
        if cursor != self.cursor {
            let old = std::mem::replace(&mut self.cursor, cursor);
            self.touch_cursor_row(old.1);
            self.touch_cursor_row(cursor.1);
        }
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

        let width = pixel_width.saturating_sub(inset_width);
        let height = pixel_height.saturating_sub(inset_height);

        Ok(ratatui::layout::Size {
            width: (width / self.fonts.min_width_px().max(1)) as u16,
            height: (height / self.fonts.height_px().max(1)) as u16,
        })
    }

    fn window_size(&mut self) -> std::io::Result<ratatui::backend::WindowSize> {
        let (inset_width, inset_height) = match self.viewport {
            Viewport::Full => (0, 0),
            Viewport::Shrink { width, height } => (width, height),
        };

        let pixel_width = self.cols as u32 * self.fonts.min_width_px();
        let pixel_height = self.rows as u32 * self.fonts.height_px();

        let width = pixel_width.saturating_sub(inset_width);
        let height = pixel_height.saturating_sub(inset_height);

        Ok(ratatui::backend::WindowSize {
            columns_rows: ratatui::layout::Size {
                width: (width / self.fonts.min_width_px().max(1)) as u16,
                height: (height / self.fonts.height_px().max(1)) as u16,
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
        self.reshape_dirty_rows();
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
    use ratatui::style::{Color, Modifier};

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
    fn style_setters_before_the_first_draw_do_not_panic() {
        let mut backend = TerminalBuilder::new(test_fonts())
            .with_dimensions(4, 3)
            .build();
        RatatuiBackend::show_cursor(&mut backend).expect("show cursor");
        RatatuiBackend::set_cursor_position(&mut backend, (1, 2)).expect("set cursor");
        backend.set_cursor_style(CursorStyle::HollowBlock);
        backend.set_dim_alpha(7);
        backend.set_palette(TerminalPalette::xterm());
        #[cfg(feature = "blink")]
        backend.set_blink_off(Modifier::SLOW_BLINK);
        backend.reshape_dirty_rows();
        let _ = backend.take_draw_payload();
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

    fn styled(symbol: &str, modifier: Modifier) -> Cell {
        let mut cell = Cell::default();
        cell.set_symbol(symbol);
        cell.modifier = modifier;
        cell
    }

    fn payload_of(
        mut backend: BevyTerminalBackend,
        cells: &[(u16, u16, Cell)],
    ) -> crate::backend::TerminalDrawPayload {
        RatatuiBackend::draw(
            &mut backend,
            cells.iter().map(|(x, y, cell)| (*x, *y, cell)),
        )
        .expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");
        backend.take_draw_payload()
    }

    fn one_row(cols: u16) -> BevyTerminalBackend {
        TerminalBuilder::new(test_fonts())
            .with_dimensions(cols, 1)
            .build()
    }

    #[test]
    fn dim_lowers_foreground_alpha() {
        let cells = [
            (0, 0, styled("a", Modifier::DIM)),
            (1, 0, styled("b", Modifier::empty())),
        ];
        let payload = payload_of(one_row(2), &cells);
        assert_eq!(payload.cells()[0].fg[3], 128);
        assert_eq!(payload.cells()[1].fg[3], 255);
    }

    #[test]
    fn dim_alpha_is_configurable() {
        let backend = TerminalBuilder::new(test_fonts())
            .with_dimensions(1, 1)
            .with_dim_alpha(77)
            .build();
        let payload = payload_of(backend, &[(0, 0, styled("a", Modifier::DIM))]);
        assert_eq!(payload.cells()[0].fg[3], 77);
    }

    #[test]
    fn dim_and_reversed_compose() {
        let mut cell = styled("a", Modifier::DIM | Modifier::REVERSED);
        cell.fg = Color::Rgb(1, 2, 3);
        cell.bg = Color::Rgb(40, 50, 60);
        let payload = payload_of(one_row(1), &[(0, 0, cell)]);
        assert_eq!(payload.cells()[0].fg, [40, 50, 60, 128]);
        assert_eq!(payload.cells()[0].bg, [1, 2, 3, 255]);
    }

    #[test]
    fn changing_dim_alpha_restyles_every_row_into_a_full_payload() {
        let mut backend = TerminalBuilder::new(test_fonts())
            .with_dimensions(1, 2)
            .build();
        let cells = [
            (0u16, 0u16, styled("a", Modifier::DIM)),
            (0, 1, styled("b", Modifier::DIM)),
        ];
        RatatuiBackend::draw(
            &mut backend,
            cells.iter().map(|(x, y, cell)| (*x, *y, cell)),
        )
        .expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");
        let _ = backend.take_draw_payload();

        backend.set_dim_alpha(40);
        let payload = backend.take_draw_payload();
        assert!(payload.is_full());
        assert!(payload.cells().iter().all(|cell| cell.fg[3] == 40));

        backend.set_dim_alpha(40);
        let payload = backend.take_draw_payload();
        assert!(payload.cells().is_empty(), "an unchanged alpha is free");
    }

    #[test]
    fn hidden_cell_draws_background_only() {
        let bg = Color::Rgb(10, 20, 30);
        let mut hidden = styled("a", Modifier::HIDDEN);
        hidden.bg = bg;
        let mut blank = styled(" ", Modifier::HIDDEN);
        blank.bg = bg;
        let hidden = payload_of(one_row(1), &[(0, 0, hidden)]);
        let blank = payload_of(one_row(1), &[(0, 0, blank)]);
        assert_eq!(
            sorted_bytes(hidden.cells()),
            sorted_bytes(blank.cells()),
            "a hidden cell is its blank twin"
        );
        assert_eq!(hidden.cells().len(), 1);
    }

    #[test]
    fn hidden_wide_cell_paints_both_cells() {
        let cw = test_fonts().min_width_px() as u16;
        let mut cell = styled("日", Modifier::HIDDEN);
        cell.bg = Color::Rgb(10, 20, 30);
        let payload = payload_of(one_row(2), &[(0, 0, cell)]);
        assert_eq!(payload.cells().len(), 2);
        let covered: u16 = payload.cells().iter().map(|cell| cell.rect[2]).sum();
        assert_eq!(covered, 2 * cw);
        assert!(
            payload
                .cells()
                .iter()
                .all(|cell| cell.bg == [10, 20, 30, 255])
        );
    }

    #[test]
    fn toggling_hidden_dirties_only_its_row() {
        let mut backend = TerminalBuilder::new(test_fonts())
            .with_dimensions(1, 3)
            .build();
        let rows = [("a", 0u16), ("b", 1), ("c", 2)]
            .map(|(symbol, y)| (0u16, y, styled(symbol, Modifier::empty())));
        RatatuiBackend::draw(&mut backend, rows.iter().map(|(x, y, cell)| (*x, *y, cell)))
            .expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");
        let _ = backend.take_draw_payload();

        let ch = backend.fonts.height_px() as u16;
        for modifier in [Modifier::HIDDEN, Modifier::empty()] {
            let cell = styled("b", modifier);
            RatatuiBackend::draw(&mut backend, [(0u16, 1u16, &cell)].into_iter())
                .expect("draw failed");
            RatatuiBackend::flush(&mut backend).expect("flush failed");
            let payload = backend.take_draw_payload();
            assert!(!payload.is_full());
            assert_eq!(payload.cells().len(), 2, "row clear + the cell");
            assert!(payload.cells().iter().all(|cell| cell.rect[1] == ch));
        }
    }

    #[test]
    fn underline_is_one_overlay_per_cell() {
        let backend = one_row(2);
        let d = backend.decorations;
        let mut a = styled("a", Modifier::UNDERLINED);
        a.fg = Color::Rgb(9, 8, 7);
        let b = styled("b", Modifier::UNDERLINED);
        let payload = payload_of(backend, &[(0, 0, a), (1, 0, b)]);
        assert_eq!(payload.overlays().len(), 2);
        let bar = payload.overlays()[0];
        assert_eq!(bar.rect[1] as u32, d.underline_top);
        assert_eq!(bar.rect[3] as u32, d.underline_thickness);
        assert_eq!(bar.fg, [9, 8, 7, 255]);
    }

    #[test]
    fn underline_wide_char_spans_two_cells() {
        let cw = test_fonts().min_width_px() as u16;
        let payload = payload_of(one_row(2), &[(0, 0, styled("日", Modifier::UNDERLINED))]);
        let xs: Vec<u16> = payload.overlays().iter().map(|bar| bar.rect[0]).collect();
        assert_eq!(xs, [0, cw]);
    }

    #[test]
    fn strikeout_and_underline_together() {
        let backend = one_row(1);
        let d = backend.decorations;
        let cell = styled("a", Modifier::UNDERLINED | Modifier::CROSSED_OUT);
        let payload = payload_of(backend, &[(0, 0, cell)]);
        let mut tops: Vec<u32> = payload
            .overlays()
            .iter()
            .map(|bar| bar.rect[1] as u32)
            .collect();
        tops.sort();
        let mut expected = [d.underline_top, d.strikeout_top];
        expected.sort();
        assert_eq!(tops, expected);
    }

    #[test]
    fn hidden_suppresses_decorations() {
        let cell = styled("a", Modifier::HIDDEN | Modifier::UNDERLINED);
        let payload = payload_of(one_row(1), &[(0, 0, cell)]);
        assert!(payload.overlays().is_empty());
    }

    #[test]
    fn undecorated_rows_add_no_overlays() {
        let cells = [
            (0, 0, styled("a", Modifier::BOLD)),
            (1, 0, styled("b", Modifier::DIM)),
        ];
        let payload = payload_of(one_row(2), &cells);
        assert!(payload.overlays().is_empty());
    }

    #[test]
    fn decoration_metrics_stay_inside_cell() {
        let font_data = include_bytes!("../../examples/assets/fonts/Mplus1Code-Regular.ttf");
        for size in [6, 8, 10, 13, 16, 24, 33, 48] {
            let fonts = Fonts::new(Font::new(font_data).expect("font"), size);
            let d = fonts.decoration_metrics();
            for (top, thickness) in [
                (d.underline_top, d.underline_thickness),
                (d.strikeout_top, d.strikeout_thickness),
            ] {
                assert!(thickness >= 1, "size {size}");
                assert!(top + thickness <= fonts.height_px(), "size {size}: {d:?}");
            }
        }
    }
    fn cursor_row(style: CursorStyle) -> BevyTerminalBackend {
        let mut backend = TerminalBuilder::new(test_fonts())
            .with_dimensions(2, 1)
            .with_cursor_style(style)
            .build();
        let mut cell = styled("a", Modifier::empty());
        cell.fg = Color::Rgb(1, 2, 3);
        cell.bg = Color::Rgb(4, 5, 6);
        RatatuiBackend::draw(&mut backend, [(0u16, 0u16, &cell)].into_iter()).expect("draw");
        backend.show_cursor().expect("show");
        backend.set_cursor_position((0u16, 0u16)).expect("move");
        RatatuiBackend::flush(&mut backend).expect("flush");
        backend
    }

    #[test]
    fn cursor_block_reverses_cell() {
        let payload = cursor_row(CursorStyle::Block).take_draw_payload();
        assert_eq!(payload.cells()[0].fg, [4, 5, 6, 255]);
        assert_eq!(payload.cells()[0].bg, [1, 2, 3, 255]);
        assert!(payload.overlays().is_empty());
    }

    #[test]
    fn cursor_hollow_is_four_overlays() {
        let payload = cursor_row(CursorStyle::HollowBlock).take_draw_payload();
        assert_eq!(payload.overlays().len(), 4);
        assert_eq!(payload.cells()[0].fg, [1, 2, 3, 255]);
        assert!(
            payload
                .overlays()
                .iter()
                .all(|bar| bar.fg == [1, 2, 3, 255])
        );
    }

    #[test]
    fn cursor_underline_and_bar_are_one_overlay() {
        for style in [CursorStyle::Underline, CursorStyle::Bar] {
            let payload = cursor_row(style).take_draw_payload();
            assert_eq!(payload.overlays().len(), 1, "{style:?}");
        }
    }

    #[test]
    fn cursor_hidden_style_draws_nothing() {
        let payload = cursor_row(CursorStyle::Hidden).take_draw_payload();
        assert!(payload.overlays().is_empty());
        assert_eq!(payload.cells()[0].fg, [1, 2, 3, 255]);
    }

    #[test]
    fn cursor_move_dirties_old_and_new_rows() {
        let mut backend = TerminalBuilder::new(test_fonts())
            .with_dimensions(1, 3)
            .build();
        let cell = styled("a", Modifier::empty());
        let cells = [(0u16, 0u16), (0, 1), (0, 2)];
        RatatuiBackend::draw(&mut backend, cells.iter().map(|&(x, y)| (x, y, &cell)))
            .expect("draw");
        RatatuiBackend::flush(&mut backend).expect("flush");
        let _ = backend.take_draw_payload();
        let ch = backend.fonts.height_px() as u16;
        let rows = |payload: &crate::backend::TerminalDrawPayload| {
            let mut ys: Vec<u16> = payload.cells().iter().map(|c| c.rect[1] / ch).collect();
            ys.dedup();
            ys
        };

        backend.show_cursor().expect("show");
        backend.set_cursor_position((0u16, 0u16)).expect("move");
        RatatuiBackend::flush(&mut backend).expect("flush");
        let payload = backend.take_draw_payload();
        assert!(!payload.is_full());
        assert_eq!(rows(&payload), [0]);

        backend.set_cursor_position((0u16, 2u16)).expect("move");
        RatatuiBackend::flush(&mut backend).expect("flush");
        assert_eq!(rows(&backend.take_draw_payload()), [0, 2]);
    }

    #[test]
    fn cursor_unchanged_does_not_dirty() {
        let backend = TerminalBuilder::new(test_fonts())
            .with_dimensions(2, 2)
            .build();
        let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| frame.set_cursor_position((1, 1)))
            .expect("draw");
        assert!(terminal.backend().cells_changed_last_draw());
        terminal
            .draw(|frame| frame.set_cursor_position((1, 1)))
            .expect("draw");
        assert!(!terminal.backend().cells_changed_last_draw());
        terminal.draw(|_| {}).expect("draw");
        assert!(terminal.backend().cells_changed_last_draw());
    }

    #[test]
    fn set_cursor_style_reshapes_only_a_shown_cursor() {
        let mut backend = cursor_row(CursorStyle::Block);
        let _ = backend.take_draw_payload();
        backend.set_cursor_style(CursorStyle::Block);
        assert!(backend.take_draw_payload().cells().is_empty());
        backend.set_cursor_style(CursorStyle::HollowBlock);
        assert_eq!(backend.take_draw_payload().overlays().len(), 4);
    }

    #[test]
    fn default_palette_keeps_current_colors() {
        let palette = TerminalPalette::default();
        let p = CellPalette {
            reset_fg: [255; 3],
            reset_bg: [0; 3],
            transparent_reset_bg: false,
            dim_alpha: 128,
            colors: palette,
        };
        assert_eq!(p.resolve(Color::Red, [0; 3]), [255, 0, 0]);
        assert_eq!(p.resolve(Color::Indexed(1), [0; 3]), [0x80, 0, 0]);
        assert_eq!(p.resolve(Color::DarkGray, [0; 3]), [169, 169, 169]);
    }

    #[test]
    fn xterm_palette_unifies_named_and_indexed() {
        let palette = TerminalPalette::xterm();
        assert_eq!(palette.named, palette.indexed[..16]);
        let themed = TerminalPalette::default().with_ansi16([[7; 3]; 16]);
        assert_eq!(themed.named, [[7; 3]; 16]);
        assert_eq!(themed.indexed[15], [7; 3]);
        assert_eq!(themed.indexed[16], TerminalPalette::default().indexed[16]);
    }

    #[test]
    fn set_palette_restyles_without_redraw() {
        let mut backend = one_row(1);
        let mut cell = styled("a", Modifier::empty());
        cell.fg = Color::Red;
        RatatuiBackend::draw(&mut backend, [(0u16, 0u16, &cell)].into_iter()).expect("draw");
        RatatuiBackend::flush(&mut backend).expect("flush");
        assert_eq!(backend.take_draw_payload().cells()[0].fg[..3], [255, 0, 0]);

        backend.set_palette(TerminalPalette::default().with_ansi16([[9, 8, 7]; 16]));
        let payload = backend.take_draw_payload();
        assert!(payload.is_full());
        assert_eq!(payload.cells()[0].fg[..3], [9, 8, 7]);
    }

    #[test]
    fn set_same_palette_is_noop() {
        let mut backend = one_row(1);
        RatatuiBackend::draw(
            &mut backend,
            [(0u16, 0u16, &styled("a", Modifier::empty()))].into_iter(),
        )
        .expect("draw");
        RatatuiBackend::flush(&mut backend).expect("flush");
        let _ = backend.take_draw_payload();
        RatatuiBackend::draw(&mut backend, std::iter::empty()).expect("draw");

        backend.set_palette(TerminalPalette::default());
        assert!(!backend.cells_changed_last_draw());
        assert!(backend.dirty_rows.iter().all(|&d| !d));
        assert!(backend.rows_dirty_since_take.iter().all(|&d| !d));
    }

    #[cfg(feature = "underline_color")]
    fn underlined(color: Color, modifier: Modifier) -> Cell {
        let mut cell = styled("a", modifier);
        cell.fg = Color::Rgb(9, 8, 7);
        cell.underline_color = color;
        cell
    }

    #[cfg(feature = "underline_color")]
    #[test]
    fn underline_color_overrides_fg() {
        let cell = underlined(Color::Rgb(1, 2, 3), Modifier::UNDERLINED);
        let payload = payload_of(one_row(1), &[(0, 0, cell)]);
        assert_eq!(payload.overlays()[0].fg, [1, 2, 3, 255]);
    }

    #[cfg(feature = "underline_color")]
    #[test]
    fn underline_color_reset_follows_fg() {
        let cell = underlined(Color::Reset, Modifier::UNDERLINED | Modifier::REVERSED);
        let payload = payload_of(one_row(1), &[(0, 0, cell)]);
        assert_eq!(payload.overlays()[0].fg, payload.cells()[0].fg);
    }

    #[cfg(feature = "underline_color")]
    #[test]
    fn strikeout_ignores_underline_color() {
        let cell = underlined(Color::Rgb(1, 2, 3), Modifier::CROSSED_OUT);
        let payload = payload_of(one_row(1), &[(0, 0, cell)]);
        assert_eq!(payload.overlays()[0].fg, [9, 8, 7, 255]);
    }

    #[cfg(feature = "underline_color")]
    #[test]
    fn dim_applies_to_underline_color() {
        let cell = underlined(Color::Rgb(1, 2, 3), Modifier::UNDERLINED | Modifier::DIM);
        let payload = payload_of(one_row(1), &[(0, 0, cell)]);
        assert_eq!(payload.overlays()[0].fg, [1, 2, 3, DEFAULT_DIM_ALPHA]);
    }

    #[test]
    fn font_glyph_preferred_over_the_procedural_fallback() {
        let backend = one_row(2);
        let fonts = Arc::clone(&backend.fonts);
        // The bundled font has `•` but no `▲`.
        let cells = [
            (0, 0, styled("•", Modifier::empty())),
            (1, 0, styled("▲", Modifier::empty())),
        ];
        let payload = payload_of(backend, &cells);
        assert_eq!(payload.cells().len(), 2);
        let (w, h) = (fonts.min_width_px(), fonts.height_px());
        let rasterized_procedurally = |ch: char| {
            let key = Key {
                style: Modifier::empty(),
                glyph: ch as u32,
                font: fonts.last_resort_id(),
                cells: 1,
            };
            fonts.with_shared_cpu_state(|shared| shared.cached.get(&key, w, h).cached())
        };
        assert!(!rasterized_procedurally('•'));
        assert!(rasterized_procedurally('▲'));
    }

    #[cfg(feature = "blink")]
    mod blink {
        use super::*;
        use std::time::Duration;

        #[test]
        fn blink_off_at_phases() {
            let off = |ms| crate::backend::blink_off_at(Duration::from_millis(ms));
            assert!(off(0).is_empty());
            assert!(off(500).contains(Modifier::SLOW_BLINK));
            assert!(!off(500).contains(Modifier::RAPID_BLINK));
            assert!(off(200).contains(Modifier::RAPID_BLINK));
            assert!(!off(200).contains(Modifier::SLOW_BLINK));
            assert!(off(2000).is_empty());
        }

        fn bytes(instance: &CellInstance) -> Vec<u8> {
            bytemuck::bytes_of(instance).to_vec()
        }

        #[test]
        fn blinking_cell_is_blank_while_off() {
            let mut backend = TerminalBuilder::new(test_fonts())
                .with_dimensions(1, 2)
                .build();
            let (blinking, steady) = (
                styled("a", Modifier::SLOW_BLINK),
                styled(" ", Modifier::empty()),
            );
            RatatuiBackend::draw(
                &mut backend,
                [(0u16, 0u16, &blinking), (0, 1, &steady)].into_iter(),
            )
            .expect("draw");
            RatatuiBackend::flush(&mut backend).expect("flush");
            let lit = backend.take_draw_payload();

            assert!(backend.blink_needs_update(Modifier::SLOW_BLINK));
            backend.set_blink_off(Modifier::SLOW_BLINK);
            let off = backend.take_draw_payload();
            assert!(!off.is_full());
            assert_eq!(off.cells().len(), 2, "clear + the blinking row only");
            let blank = &lit.cells()[1];
            let drawn = &off.cells()[1];
            assert_eq!(
                (drawn.uv, drawn.fg, drawn.bg),
                (blank.uv, blank.fg, blank.bg)
            );
            assert_ne!(drawn.uv, lit.cells()[0].uv);

            backend.set_blink_off(Modifier::empty());
            let on = backend.take_draw_payload();
            assert_eq!(bytes(&on.cells()[1]), bytes(&lit.cells()[0]));
        }

        #[test]
        fn blink_without_blinking_rows_is_noop() {
            let mut backend = one_row(1);
            RatatuiBackend::draw(
                &mut backend,
                [(0u16, 0u16, &styled("a", Modifier::empty()))].into_iter(),
            )
            .expect("draw");
            RatatuiBackend::flush(&mut backend).expect("flush");
            for off in [
                Modifier::empty(),
                Modifier::SLOW_BLINK,
                Modifier::RAPID_BLINK,
                Modifier::SLOW_BLINK | Modifier::RAPID_BLINK,
            ] {
                assert!(!backend.blink_needs_update(off));
            }
        }

        #[test]
        fn a_shown_cursor_blinks_with_the_slow_phase() {
            let mut backend = cursor_row(CursorStyle::Block);
            let lit = backend.take_draw_payload();
            assert!(backend.blink_needs_update(Modifier::SLOW_BLINK));
            backend.set_blink_off(Modifier::SLOW_BLINK);
            let off = backend.take_draw_payload();
            assert_ne!(bytes(off.cells().last().unwrap()), bytes(&lit.cells()[0]));
            assert!(!backend.blink_needs_update(Modifier::SLOW_BLINK));
        }
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
    fn hidden_emoji_is_blank() {
        for emoji in FIXTURES {
            let mut backend = TerminalBuilder::new(fonts(emoji))
                .with_dimensions(2, 1)
                .build();
            let mut cell = Cell::default();
            cell.set_symbol("\u{1F600}");
            cell.modifier = ratatui::style::Modifier::HIDDEN;
            RatatuiBackend::draw(&mut backend, [(0u16, 0u16, &cell)].into_iter())
                .expect("draw failed");
            RatatuiBackend::flush(&mut backend).expect("flush failed");
            let payload = backend.take_draw_payload();
            assert!(payload.colors().is_empty());
            assert_eq!(payload.cells().len(), 2);
        }
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
                    palette: &CellPalette {
                        reset_fg: [255; 3],
                        reset_bg: [0; 3],
                        transparent_reset_bg: false,
                        dim_alpha: DEFAULT_DIM_ALPHA,
                        colors: TerminalPalette::default(),
                    },
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
