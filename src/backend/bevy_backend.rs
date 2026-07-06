use std::collections::HashSet;
use std::sync::Arc;
use web_time::{Duration, Instant};

use crate::backend::rasterize::rasterize_glyph;
use crate::backend::TextBgVertexMember;
use crate::backend::TextVertexMember;
use crate::backend::Viewport;
use crate::colors::Rgb;
use crate::fonts::Fonts;
use crate::utils::plan_cache::PlanCache;
use crate::utils::text_atlas::Atlas;
use crate::utils::text_atlas::CacheRect;
use crate::utils::text_atlas::Key;
use crate::RandomState;
use bitvec::vec::BitVec;
use indexmap::IndexMap;
use ratatui::buffer::Cell;
use ratatui::text::Line;
use rustybuzz::ttf_parser::GlyphId;
use rustybuzz::UnicodeBuffer;

#[allow(dead_code)]
const NULL_CELL: Cell = Cell::new("");

#[allow(dead_code)]
pub(super) struct RenderInfo {
    cell: usize,
    cached: CacheRect,
    underline_pos_min: u16,
    underline_pos_max: u16,
}

/// Map from (x, y, glyph) -> (cell index, cache entry).
type Rendered = IndexMap<(i32, i32, GlyphId), RenderInfo, RandomState>;

/// Set of (x, y, glyph, char width).
type Sourced = HashSet<(i32, i32, GlyphId, u32), RandomState>;

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
    pub(super) dirty_cells: BitVec,
    /// Set by `draw()` when ratatui's internal buffer diff contained at
    /// least one changed cell; cleared at the start of every `draw()` call.
    /// Lets `Tui::draw` skip marking itself dirty (and thus skip the GPU
    /// render + copy) when a redraw produces byte-identical content.
    pub(super) cells_changed_last_draw: bool,
    pub(super) cursor: (u16, u16),
    pub(super) viewport: Viewport,

    // ====== Rendering state ======
    pub(super) rendered: Vec<Rendered>,
    pub(super) sourced: Vec<Sourced>,

    // ====== Font management (Arc, no lifetime) ======
    pub(super) fonts: Arc<Fonts>,
    pub(super) plan_cache: PlanCache,
    pub(super) buffer: UnicodeBuffer,
    pub(super) row: String,
    pub(super) rowmap: Vec<u16>,

    // ====== Glyph cache (owned) ======
    // Slot allocation only (a pure CPU LRU tracker, no wgpu dependency) -
    // the corresponding GPU texture lives in the render-world
    // `TerminalGpuState`, keyed by destination image.
    pub(super) cached: Atlas,

    // ====== Draw data (owned) ======
    pub(super) bg_vertices: Vec<TextBgVertexMember>,
    pub(super) text_indices: Vec<[u32; 6]>,
    pub(super) text_vertices: Vec<TextVertexMember>,

    // ====== Pending GPU uploads ======
    pub(super) pending_cache_updates: Vec<(CacheRect, Vec<u32>, bool)>,

    // ====== Color settings ======
    pub(super) reset_fg: Rgb,
    pub(super) reset_bg: Rgb,
    /// If true, a cell whose *effective* background is `Color::Reset`
    /// (accounting for `Modifier::REVERSED` - see the color-selection logic
    /// in `flush()`) is packed with alpha 0 instead of 255, making it show
    /// through to whatever is behind the terminal's surface (a world-quad
    /// with `AlphaMode::Blend`, or a transparent-background UI node).
    /// Localized entirely to vertex-color packing here - no shader/pipeline
    /// change needed (`composite_bg.wgsl` already unpacks and outputs the
    /// full RGBA of `bg_color`, and the bg pipeline's `BlendState::REPLACE`
    /// writes it through as-is).
    pub(super) transparent_reset_bg: bool,
    /// Color shown before any content has been drawn (or while nothing is
    /// drawn at all - e.g. mid-resize): the render world's `LoadOp::Clear`
    /// color whenever a draw payload's vertex data is empty. Carried on
    /// every `TerminalDrawPayload` as `clear_color` (see `take_draw_payload`).
    pub(super) initial_fill: [u8; 4],

    // ====== Blink management (for future use) ======
    #[allow(dead_code)]
    pub(super) fast_blinking: BitVec,
    #[allow(dead_code)]
    pub(super) slow_blinking: BitVec,
    #[allow(dead_code)]
    pub(super) fast_duration: Duration,
    #[allow(dead_code)]
    pub(super) last_fast_toggle: Instant,
    #[allow(dead_code)]
    pub(super) show_fast: bool,
    #[allow(dead_code)]
    pub(super) slow_duration: Duration,
    #[allow(dead_code)]
    pub(super) last_slow_toggle: Instant,
    #[allow(dead_code)]
    pub(super) show_slow: bool,
}

/// Builder for BevyTerminalBackend. Fully synchronous, requires Device/Queue at build().
pub struct TerminalBuilder {
    fonts: Arc<Fonts>,
    cols: u16,
    rows: u16,
    reset_fg: Rgb,
    reset_bg: Rgb,
    viewport: Viewport,
    fast_blink: Duration,
    slow_blink: Duration,
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
            fast_blink: Duration::from_millis(200),
            slow_blink: Duration::from_millis(1000),
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
    /// pipelines) are created lazily in the render world on this
    /// terminal's first extract (see `TerminalGpuState` in `backend/mod.rs`
    /// and the render-world store in `bevy_plugin.rs`).
    pub fn build(self) -> BevyTerminalBackend {
        use crate::backend::{CACHE_HEIGHT, CACHE_WIDTH};

        // Initialize Atlas (pure CPU slot allocator - no wgpu dependency)
        let cached = Atlas::new(&self.fonts, CACHE_WIDTH, CACHE_HEIGHT);

        // Initialize plan cache
        let plan_cache = PlanCache::new(self.fonts.count().max(2));

        // Initialize blink timers
        let now = Instant::now();

        BevyTerminalBackend {
            cols: self.cols,
            rows: self.rows,
            cells: vec![],
            dirty_rows: vec![],
            dirty_cells: BitVec::new(),
            cells_changed_last_draw: false,
            cursor: (0, 0),
            viewport: self.viewport,
            rendered: vec![],
            sourced: vec![],
            fonts: self.fonts,
            plan_cache,
            buffer: UnicodeBuffer::new(),
            row: String::new(),
            rowmap: vec![],
            cached,
            bg_vertices: vec![],
            text_indices: vec![],
            text_vertices: vec![],
            pending_cache_updates: vec![],
            reset_fg: self.reset_fg,
            reset_bg: self.reset_bg,
            transparent_reset_bg: self.transparent_reset_bg,
            initial_fill: self.initial_fill,
            fast_blinking: BitVec::new(),
            slow_blinking: BitVec::new(),
            fast_duration: self.fast_blink,
            last_fast_toggle: now,
            show_fast: true,
            slow_duration: self.slow_blink,
            last_slow_toggle: now,
            show_slow: true,
        }
    }
}

/// Convert tiny-skia Pixmap to Vec<u32> in RGBA8 format
fn pixmap_to_rgba8(pixmap: tiny_skia::Pixmap) -> Vec<u32> {
    pixmap
        .data()
        .chunks_exact(4)
        .map(|rgba| {
            // tiny-skia uses RGBA byte order
            let r = rgba[0];
            let g = rgba[1];
            let b = rgba[2];
            let a = rgba[3];
            // Pack into u32 (little-endian RGBA)
            u32::from_le_bytes([r, g, b, a])
        })
        .collect()
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
    /// `dirty_rows`, `sourced`, `rendered`, blink bitvecs) are resized
    /// lazily inside `draw()` based on the bounds this affects, the same
    /// path a fresh terminal's first draw already goes through. Callers
    /// must also resize ratatui's own front/back buffers (`Terminal::resize`
    /// or the diff-driving `autoresize` on the next `Terminal::draw`) - this
    /// method only updates what `BevyTerminalBackend` owns directly.
    pub(crate) fn resize(&mut self, cols: u16, rows: u16) {
        self.cols = cols;
        self.rows = rows;
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

            // Convert tiny-skia Pixmap to Vec<u32> (RGBA8 format)
            let bitmap = pixmap_to_rgba8(pixmap);

            // Create atlas key
            let key = Key {
                style: Modifier::empty(),
                glyph: unicode_char as u32,
                font: font_id,
            };

            // Get atlas slot (this allocates space in the atlas)
            let rect = self.cached.get(&key, width, height);

            // Queue for GPU upload
            self.pending_cache_updates.push((*rect, bitmap, false));

            populated_count += 1;
        }

        tracing::debug!(
            "Successfully pre-populated {} programmatic glyphs ({} skipped - not yet implemented)",
            populated_count,
            skipped_count
        );
    }

    /// Drain the CPU-computed draw payload for the render world to consume:
    /// pending glyph rasterizations plus the background/foreground
    /// vertex+index data ratatui's diffed buffer produced on the most
    /// recent `draw()`/`flush()`. Called only when the terminal is dirty
    /// (see `Tui::flush` in `setup.rs`) - the render-world side
    /// (`TerminalGpuState::render` in `backend/mod.rs`) does the actual GPU
    /// work.
    pub(crate) fn take_draw_payload(&mut self) -> crate::backend::TerminalDrawPayload {
        use ratatui::backend::Backend;

        let bounds = Backend::size(self).unwrap_or(ratatui::layout::Size {
            width: 0,
            height: 0,
        });

        crate::backend::TerminalDrawPayload {
            screen_width_px: bounds.width as f32 * self.fonts.min_width_px() as f32,
            screen_height_px: bounds.height as f32 * self.fonts.height_px() as f32,
            clear_color: self.initial_fill,
            glyph_uploads: std::mem::take(&mut self.pending_cache_updates),
            bg_vertices: std::mem::take(&mut self.bg_vertices),
            text_vertices: std::mem::take(&mut self.text_vertices),
            text_indices: std::mem::take(&mut self.text_indices),
        }
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
        // Invalidate caches and mark all dirty
        self.dirty_rows.clear();
        self.cached.match_fonts(&new_fonts);
        self.fonts = new_fonts;
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
        self.sourced.resize_with(
            bounds.height as usize * bounds.width as usize,
            Sourced::default,
        );
        self.rendered.resize_with(
            bounds.height as usize * bounds.width as usize,
            Rendered::default,
        );
        self.fast_blinking
            .resize(bounds.height as usize * bounds.width as usize, false);
        self.slow_blinking
            .resize(bounds.height as usize * bounds.width as usize, false);
        self.dirty_rows.resize(bounds.height as usize, true);
        self.cells_changed_last_draw = false;

        for (x, y, cell) in content {
            self.cells_changed_last_draw = true;
            let index = y as usize * bounds.width as usize + x as usize;

            self.fast_blinking.set(
                index,
                cell.modifier
                    .contains(ratatui::style::Modifier::RAPID_BLINK),
            );
            self.slow_blinking.set(
                index,
                cell.modifier.contains(ratatui::style::Modifier::SLOW_BLINK),
            );

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
        self.cursor = (0, 0);

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

    /// Prepare vertex data for GPU rendering.
    ///
    /// This method performs the following operations:
    /// 1. Text shaping: Converts cell text into shaped glyphs using rustybuzz
    /// 2. Glyph caching: Rasterizes new glyphs and caches them in the GPU texture atlas
    /// 3. Vertex generation: Creates background and foreground vertices for rendering
    ///
    /// The generated vertices are stored in:
    /// - `bg_vertices`: Background quad vertices (one quad per glyph)
    /// - `text_vertices`: Foreground text vertices with UV coords into glyph atlas
    /// - `text_indices`: Index buffer for efficient quad rendering
    ///
    /// Pending glyph rasterizations are stored in `pending_cache_updates` and will be
    /// uploaded to the GPU texture by the render world (see `take_draw_payload`).
    ///
    /// # Performance Notes
    ///
    /// Currently marks all cells as dirty for simplicity. Future optimization:
    /// track only changed cells based on `dirty_rows` from `draw()`.
    fn flush(&mut self) -> std::io::Result<()> {
        use crate::backend::c2c;
        use rustybuzz::shape_with_plan;
        use rustybuzz::ttf_parser::GlyphId;

        let bounds = self.size()?;

        // Clear the vertex/index buffers - they are full-frame state,
        // regenerated from scratch below. `pending_cache_updates` must NOT
        // be cleared here: its entries are one-shot atlas uploads for slots
        // the `Atlas` LRU already considers cached, queued by a *previous*
        // flush (or by `populate_programmatic_glyphs` at creation) and not
        // yet drained by `take_draw_payload`. Clearing them here destroys
        // the only copy of those glyphs' pixels - the glyph is never
        // re-rasterized (the LRU says it's cached) and its slot stays
        // garbage forever, i.e. permanently invisible characters. This is
        // exactly what happens when `Terminal::draw` runs more than once
        // between payload-takes: creation-time populate + a setup-time
        // initial draw + the first per-frame draw are three flushes before
        // the first `gpu_flush_system` ever runs.
        self.bg_vertices.clear();
        self.text_vertices.clear();
        self.text_indices.clear();

        // Mark all cells as dirty for now (TODO: optimize)
        self.dirty_cells.clear();
        self.dirty_cells.resize(self.cells.len(), true);

        // Process each row
        let mut index_offset = 0;
        for y in 0..bounds.height as usize {
            let row_start = y * bounds.width as usize;
            let row_end = (row_start + bounds.width as usize).min(self.cells.len());
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
                continue;
            }

            // Shape the row
            let mut buffer = std::mem::take(&mut self.buffer);
            buffer.clear();
            for (idx, ch) in self.row.char_indices() {
                buffer.add(ch, idx as u32);
            }

            // For now, use font_for_cell on the first cell
            #[cfg(feature = "bold_italic_fonts")]
            let (font, _cell_fake_bold, _cell_fake_italic) =
                self.fonts.font_for_cell(&row_cells[0]);

            #[cfg(not(feature = "bold_italic_fonts"))]
            let (font, _cell_fake_bold, _cell_fake_italic) = {
                let (f, _, _) = self.fonts.font_for_cell(&row_cells[0]);
                (f, false, false) // Disable fake styling when feature is off
            };

            let glyph_buffer =
                shape_with_plan(font.font(), self.plan_cache.get(font, &mut buffer), buffer);

            let infos = glyph_buffer.glyph_infos();
            let positions = glyph_buffer.glyph_positions();

            // Process shaped glyphs
            let metrics = font.font();
            let advance_scale = self.fonts.height_px() as f32 / metrics.height() as f32;

            for (info, pos) in infos.iter().zip(positions.iter()) {
                let cluster = info.cluster as usize;
                if cluster >= self.rowmap.len() {
                    continue;
                }

                let cell_idx = self.rowmap[cluster] as usize;
                if cell_idx >= row_cells.len() {
                    continue;
                }

                let cell = &row_cells[cell_idx];
                let _glyph_id = GlyphId(info.glyph_id as u16);

                // Use per-cell font selection for proper styling
                #[cfg(feature = "bold_italic_fonts")]
                let (cell_font, cell_fake_bold, cell_fake_italic) = self.fonts.font_for_cell(cell);

                #[cfg(not(feature = "bold_italic_fonts"))]
                let (cell_font, cell_fake_bold, cell_fake_italic) = (font, false, false);

                // Calculate character width using unicode-width for precise glyph width
                use unicode_width::UnicodeWidthChar;
                let ch = cell.symbol().chars().next().unwrap_or(' ');
                let ch_width = ch.width().unwrap_or(1).max(1) as u32;
                let glyph_width_px = ch_width * self.fonts.min_width_px();

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
                let style = cell.modifier
                    & (ratatui::style::Modifier::BOLD | ratatui::style::Modifier::ITALIC);

                #[cfg(not(feature = "bold_italic_fonts"))]
                let style = ratatui::style::Modifier::empty();

                let key = if is_programmatic {
                    Key {
                        style,
                        glyph: ch as u32,
                        font: self.fonts.last_resort_id(),
                    }
                } else {
                    Key {
                        style,
                        glyph: info.glyph_id,
                        font: cell_font.id(),
                    }
                };

                let cached = self
                    .cached
                    .get(&key, glyph_width_px, self.fonts.height_px());

                // If not cached, render the glyph
                if !cached.cached() {
                    if is_programmatic {
                        // Render programmatic glyph on-demand if not pre-cached
                        use crate::backend::programmatic_glyphs::render_programmatic_glyph;

                        if let Some(pixmap) =
                            render_programmatic_glyph(ch, glyph_width_px, self.fonts.height_px())
                        {
                            let bitmap = pixmap_to_rgba8(pixmap);
                            self.pending_cache_updates.push((*cached, bitmap, false));
                        } else {
                            tracing::warn!(
                                "Failed to render programmatic glyph '{}' (U+{:04X})",
                                ch,
                                ch as u32
                            );
                        }
                    } else {
                        // Calculate glyph bearing offset to apply during rasterization
                        let bearing_offset_x = pos.x_offset as f32 * advance_scale;

                        // Don't apply fake styling to emoji characters to avoid distortion
                        let final_fake_italic = cell_fake_italic && !is_emoji;
                        let final_fake_bold = cell_fake_bold && !is_emoji;

                        let (rect, image) = rasterize_glyph(
                            cached,
                            metrics,
                            info,
                            final_fake_italic, // Don't distort emoji
                            final_fake_bold,   // Don't distort emoji
                            advance_scale,
                            glyph_width_px,   // Use actual glyph width
                            bearing_offset_x, // Apply offset in atlas
                        );

                        self.pending_cache_updates.push((rect, image, false));
                    }
                }

                // Calculate screen position - align to cell grid since offset is already in atlas
                let screen_x = cell_idx as f32 * self.fonts.min_width_px() as f32;
                let screen_y = y as f32 * self.fonts.height_px() as f32;

                // Get colors
                let reverse = cell.modifier.contains(ratatui::style::Modifier::REVERSED);
                // The color actually being used *as the background* -
                // `cell.fg` when reversed, matching the swap below. Checked
                // against `Color::Reset` before `c2c()` resolves it to an
                // opaque RGB, since that resolution is exactly what erases
                // the "this cell has no explicit background" information
                // `transparent_reset_bg` needs.
                let bg_source = if reverse { cell.fg } else { cell.bg };
                let bg_color = if reverse {
                    c2c(cell.fg, self.reset_fg)
                } else {
                    c2c(cell.bg, self.reset_bg)
                };
                let fg_color = if reverse {
                    c2c(cell.bg, self.reset_bg)
                } else {
                    c2c(cell.fg, self.reset_fg)
                };

                let bg_alpha = if self.transparent_reset_bg
                    && matches!(bg_source, ratatui::style::Color::Reset)
                {
                    0
                } else {
                    255
                };
                let [r, g, b] = bg_color;
                let bg_color_u32 = u32::from_be_bytes([r, g, b, bg_alpha]);

                let [r, g, b] = fg_color;
                let fg_color_u32 = u32::from_be_bytes([r, g, b, 255]);

                // Generate indices
                self.text_indices.push([
                    index_offset,
                    index_offset + 1,
                    index_offset + 2,
                    index_offset + 2,
                    index_offset + 3,
                    index_offset + 1,
                ]);
                index_offset += 4;

                // Render at actual glyph width (no compression)
                let render_width_px = glyph_width_px as f32;

                // Background vertices
                self.bg_vertices.push(TextBgVertexMember {
                    vertex: [screen_x, screen_y],
                    bg_color: bg_color_u32,
                });
                self.bg_vertices.push(TextBgVertexMember {
                    vertex: [screen_x + render_width_px, screen_y],
                    bg_color: bg_color_u32,
                });
                self.bg_vertices.push(TextBgVertexMember {
                    vertex: [screen_x, screen_y + self.fonts.height_px() as f32],
                    bg_color: bg_color_u32,
                });
                self.bg_vertices.push(TextBgVertexMember {
                    vertex: [
                        screen_x + render_width_px,
                        screen_y + self.fonts.height_px() as f32,
                    ],
                    bg_color: bg_color_u32,
                });

                // Text vertices - 1:1 mapping between atlas and screen
                let uv_x = cached.x as f32;
                let uv_y = cached.y as f32;
                let uv_w = cached.width as f32; // Matches glyph_width_px
                let uv_h = cached.height as f32;

                self.text_vertices.push(TextVertexMember {
                    vertex: [screen_x, screen_y],
                    uv: [uv_x, uv_y],
                    fg_color: fg_color_u32,
                    underline_pos: 0,
                    underline_color: fg_color_u32,
                });
                self.text_vertices.push(TextVertexMember {
                    vertex: [screen_x + render_width_px, screen_y],
                    uv: [uv_x + uv_w, uv_y],
                    fg_color: fg_color_u32,
                    underline_pos: 0,
                    underline_color: fg_color_u32,
                });
                self.text_vertices.push(TextVertexMember {
                    vertex: [screen_x, screen_y + self.fonts.height_px() as f32],
                    uv: [uv_x, uv_y + uv_h],
                    fg_color: fg_color_u32,
                    underline_pos: 0,
                    underline_color: fg_color_u32,
                });
                self.text_vertices.push(TextVertexMember {
                    vertex: [
                        screen_x + render_width_px,
                        screen_y + self.fonts.height_px() as f32,
                    ],
                    uv: [uv_x + uv_w, uv_y + uv_h],
                    fg_color: fg_color_u32,
                    underline_pos: 0,
                    underline_color: fg_color_u32,
                });
            }

            // Restore buffer (clear GlyphBuffer back to UnicodeBuffer)
            self.buffer = glyph_buffer.clear();
        }

        Ok(())
    }

    fn clear_region(
        &mut self,
        _clear_type: ratatui::backend::ClearType,
    ) -> Result<(), Self::Error> {
        // For now, just delegate to clear() for all clear types
        self.clear()
    }
}

// ============================================================================
// Test: cell-level transparency (P2-2). Pure CPU - drives ratatui's
// `Backend::draw()` directly and inspects the packed vertex color, no
// GPU/App needed. `bg_color`'s alpha byte is what `composite_bg.wgsl`
// outputs unmodified (`BlendState::REPLACE`), so this is the whole story:
// no shader/pipeline test is needed on top of this.
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts::{Font, Fonts};
    use ratatui::backend::Backend as RatatuiBackend;
    use ratatui::buffer::Cell;
    use ratatui::style::Color;

    fn test_fonts() -> Arc<Fonts> {
        let font_data = include_bytes!("../../assets/fonts/Mplus1Code-Regular.ttf");
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

        assert_eq!(
            backend.bg_vertices.len(),
            8,
            "one quad (4 vertices) per cell, 2 cells"
        );
        for vertex in &backend.bg_vertices[0..4] {
            assert_eq!(
                vertex.bg_color & 0xFF,
                0,
                "a Color::Reset background must pack alpha 0 when \
                 transparent_reset_bg is enabled"
            );
        }
        for vertex in &backend.bg_vertices[4..8] {
            assert_eq!(
                vertex.bg_color & 0xFF,
                255,
                "an explicit background color must stay fully opaque"
            );
        }
    }

    #[test]
    fn transparent_reset_bg_disabled_keeps_reset_cells_opaque() {
        let mut backend = TerminalBuilder::new(test_fonts())
            .with_dimensions(1, 1)
            .with_transparent_reset_bg(false) // the default
            .build();

        let mut reset_cell = Cell::default();
        reset_cell.set_symbol("a");
        reset_cell.bg = Color::Reset;

        RatatuiBackend::draw(&mut backend, [(0u16, 0u16, &reset_cell)].into_iter())
            .expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");

        assert!(!backend.bg_vertices.is_empty(), "flush must generate vertices");
        for vertex in &backend.bg_vertices {
            assert_eq!(
                vertex.bg_color & 0xFF,
                255,
                "without transparent_reset_bg, Reset backgrounds render \
                 opaque (using reset_bg), matching pre-P2-2 behavior"
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

        RatatuiBackend::draw(&mut backend, [(0u16, 0u16, &cell)].into_iter())
            .expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");

        assert!(!backend.bg_vertices.is_empty(), "flush must generate vertices");
        for vertex in &backend.bg_vertices {
            assert_eq!(
                vertex.bg_color & 0xFF,
                0,
                "reversed cell.fg == Reset must still zero the background alpha"
            );
        }
    }
}
