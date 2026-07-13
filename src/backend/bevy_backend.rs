use std::sync::Arc;

use crate::backend::rasterize::rasterize_glyph;
use crate::backend::TextBgVertexMember;
use crate::backend::TextVertexMember;
use crate::backend::Viewport;
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
/// `BevyTerminalBackend::flush` and IMPROVEMENT.md A2. Vertex positions
/// are absolute screen pixel coordinates (including `screen_y`, which
/// only depends on the row index, not on anything about the current
/// frame), so a cached row's vertices can be appended to the frame's
/// output verbatim with no rebasing.
#[derive(Default, Clone)]
struct RowGeometry {
    bg_vertices: Vec<TextBgVertexMember>,
    text_vertices: Vec<TextVertexMember>,
    /// `Atlas::generation()` as of when this geometry was (re)computed.
    /// A mismatch against the atlas's current generation means some slot
    /// referenced by `text_vertices`' UVs may have been reassigned to a
    /// different glyph since - the row must be treated as dirty and
    /// regenerated rather than reused.
    atlas_generation: u64,
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

    // ====== Font management (Arc, no lifetime) ======
    // `cached` (Atlas), `plan_cache`, and `pending_cache_updates` used to
    // live here, one independent copy per terminal. They now live in
    // `fonts.shared_cpu_state` instead (reachable via
    // `Fonts::with_shared_cpu_state`) - shared by every terminal that
    // holds the same `Arc<Fonts>`, so a glyph is rasterized and queued for
    // GPU upload once per font, not once per terminal (IMPROVEMENT.md C3).
    pub(super) fonts: Arc<Fonts>,
    pub(super) buffer: UnicodeBuffer,
    pub(super) row: String,
    pub(super) rowmap: Vec<u16>,
    /// cmap lookup cache for the ASCII fast path (IMPROVEMENT.md A3,
    /// feature `ascii_fast_shaping`) - `None` means "checked, this font
    /// has no glyph for this char", cached to avoid re-probing every
    /// frame. Keyed by `(font id, char)` since a terminal's font can
    /// change via `update_fonts` (which clears this) and, in principle,
    /// differ row to row if multiple fonts were ever in play.
    #[cfg(feature = "ascii_fast_shaping")]
    pub(super) ascii_glyph_cache:
        std::collections::HashMap<(u64, char), Option<rustybuzz::ttf_parser::GlyphId>>,

    // ====== Draw data (owned) ======
    // No index buffer here: quad indices are a pure function of vertex
    // count (quad i -> [4i, 4i+1, 4i+2, 4i+2, 4i+3, 4i+1]), so this
    // terminal's `TerminalGpuState` (backend/mod.rs) owns one static,
    // grow-only index buffer instead of regenerating one here every frame.
    pub(super) bg_vertices: Vec<TextBgVertexMember>,
    pub(super) text_vertices: Vec<TextVertexMember>,
    /// Per-row geometry cache, indexed by row `y`, letting `flush()` skip
    /// reshaping+re-rasterizing rows `dirty_rows` says are unchanged. See
    /// `RowGeometry` and IMPROVEMENT.md A2.
    row_geometry: Vec<RowGeometry>,

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
}

/// Builder for BevyTerminalBackend. Fully synchronous, requires Device/Queue at build().
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
    /// LRU/plan cache lazily on this `Fonts`' first use (see
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
            fonts: self.fonts,
            buffer: UnicodeBuffer::new(),
            row: String::new(),
            rowmap: vec![],
            #[cfg(feature = "ascii_fast_shaping")]
            ascii_glyph_cache: std::collections::HashMap::new(),
            bg_vertices: vec![],
            text_vertices: vec![],
            row_geometry: vec![],
            reset_fg: self.reset_fg,
            reset_bg: self.reset_bg,
            transparent_reset_bg: self.transparent_reset_bg,
            initial_fill: self.initial_fill,
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
    /// `dirty_rows`) are resized lazily inside `draw()` based on the bounds
    /// this affects, the same path a fresh terminal's first draw already
    /// goes through. Callers
    /// must also resize ratatui's own front/back buffers (`Terminal::resize`
    /// or the diff-driving `autoresize` on the next `Terminal::draw`) - this
    /// method only updates what `BevyTerminalBackend` owns directly.
    pub(crate) fn resize(&mut self, cols: u16, rows: u16) {
        self.cols = cols;
        self.rows = rows;

        // A row's cached geometry was computed for the OLD grid width -
        // row `y` may now map entirely different content (even if the
        // corresponding ratatui cells happen to diff as "unchanged" in
        // isolation). Clearing (rather than resizing) `dirty_rows` here
        // means the next `draw()`'s `resize(new_height, true)` treats
        // every row as newly added, marking all of them dirty (see
        // IMPROVEMENT.md A2's invalidation list).
        self.dirty_rows.clear();
        self.row_geometry.clear();
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

            // Get atlas slot (this allocates space in the shared atlas -
            // shared with every other terminal using this same `Fonts`,
            // IMPROVEMENT.md C3) and queue the bitmap for GPU upload.
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

    /// Drain the CPU-computed draw payload for the render world to consume:
    /// the background/foreground vertex data ratatui's diffed buffer
    /// produced on the most recent `draw()`/`flush()`, plus this backend's
    /// font identity (`font_key`, see [`Fonts::identity`]) so the render
    /// world knows which shared atlas/pipelines to render against
    /// (IMPROVEMENT.md C3). Called only when the terminal is dirty (see
    /// `Tui::flush` in `setup.rs`).
    ///
    /// Glyph rasterizations are NOT part of this payload - they queue in
    /// the shared `Fonts::with_shared_cpu_state`, keyed by font rather than
    /// by terminal, and are drained separately (once per font per frame,
    /// not once per terminal) via [`Self::take_shared_glyph_uploads`].
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
            font_key: self.fonts.identity(),
            bg_vertices: std::mem::take(&mut self.bg_vertices),
            text_vertices: std::mem::take(&mut self.text_vertices),
        }
    }

    /// This backend's font identity - see [`Fonts::identity`] and
    /// IMPROVEMENT.md C3.
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
    ) -> Vec<(crate::utils::text_atlas::CacheRect, Vec<u32>)> {
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
        // atlas invalidation needed here: the atlas now lives in
        // `Fonts::with_shared_cpu_state` (IMPROVEMENT.md C3), keyed by the
        // `Fonts` instance itself - switching to a different `Arc<Fonts>`
        // means the next flush naturally looks up (or lazily creates) that
        // OTHER font's own shared state, leaving whatever the old `Fonts`
        // owned untouched (and simply dropped once its last `Arc` clone
        // goes away, same as any other Rust value).
        self.dirty_rows.clear();
        self.row_geometry.clear();
        #[cfg(feature = "ascii_fast_shaping")]
        self.ascii_glyph_cache.clear();
        self.fonts = new_fonts;
    }

    /// Shapes and rasterizes-as-needed a single row (index `y`, of a grid
    /// `width` cells wide), returning its background and foreground
    /// vertex geometry. Factored out of `flush()` so the same per-row
    /// logic can (re)build either a freshly-dirty row or a clean row
    /// whose cache turned out to be stale (see IMPROVEMENT.md A2) -
    /// callers append the result to both this frame's output and this
    /// row's `row_geometry` cache entry. An empty row (no glyphs) returns
    /// two empty `Vec`s, which is a perfectly valid cache entry - nothing
    /// distinguishes "cached and empty" from "never cached", since a
    /// dirty row is always (re)shaped before ever being read as clean.
    fn shape_row(
        &mut self,
        y: usize,
        width: usize,
        shared: &mut crate::fonts::SharedFontCpuState,
    ) -> (Vec<TextBgVertexMember>, Vec<TextVertexMember>) {
        use crate::backend::c2c;
        use rustybuzz::shape_with_plan;
        use rustybuzz::ttf_parser::GlyphId;

        let mut bg_vertices = Vec::new();
        let mut text_vertices = Vec::new();

        // Packed the same way as `bg_color_u32` below - a bg quad whose
        // color exactly matches this is redundant (IMPROVEMENT.md B3):
        // the render pass already clears the target to `initial_fill`, so
        // drawing an identical quad on top changes nothing. This also
        // correctly keeps (never skips) an alpha-0 `transparent_reset_bg`
        // quad whenever `initial_fill`'s own alpha isn't 0 - the packed
        // values simply won't be equal in that case.
        let initial_fill_u32 = u32::from_be_bytes(self.initial_fill);

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
            return (bg_vertices, text_vertices);
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

        // ASCII fast path (IMPROVEMENT.md A3, opt-in via the
        // `ascii_fast_shaping` feature - compiled out entirely otherwise,
        // and requires `bold_italic_fonts` to be off, since that feature
        // makes per-cell font selection meaningful while this path assumes
        // the whole row shares `font`, exactly like the slow path already
        // does whenever `bold_italic_fonts` is off - see `cell_font`
        // below). Builds synthetic glyph info/position pairs straight from
        // the font's cmap instead of invoking rustybuzz at all, then feeds
        // them into the SAME per-glyph loop below as real shaped output -
        // no duplication of the atlas/vertex-emission logic, so there is
        // exactly one code path that can get that part wrong. Bails to the
        // full shaping path (`None`) the moment any cell isn't a single
        // ASCII printable byte or the font lacks that glyph.
        #[cfg(all(feature = "ascii_fast_shaping", not(feature = "bold_italic_fonts")))]
        let fast_path: Option<(Vec<rustybuzz::GlyphInfo>, Vec<rustybuzz::GlyphPosition>)> = 'fast: {
            use rustybuzz::ttf_parser::Face;

            let mut infos = Vec::with_capacity(row_cells.len());
            let mut positions = Vec::with_capacity(row_cells.len());
            for (x, cell) in row_cells.iter().enumerate() {
                let bytes = cell.symbol().as_bytes();
                // Exactly one ASCII printable byte - this single check also
                // excludes an empty symbol (NULL_CELL wide-char
                // continuation) and any multi-byte UTF-8 grapheme cluster.
                if bytes.len() != 1 || !(0x20..=0x7E).contains(&bytes[0]) {
                    break 'fast None;
                }
                let ch = bytes[0] as char;

                let cache_key = (font.id(), ch);
                let glyph_id = *self
                    .ascii_glyph_cache
                    .entry(cache_key)
                    .or_insert_with(|| Face::glyph_index(font.font(), ch));
                let Some(glyph_id) = glyph_id else {
                    break 'fast None; // font lacks this glyph - defer to full shaping
                };

                // Struct-literal update syntax (`..Default::default()`)
                // isn't usable here: `GlyphInfo` has private fields not
                // nameable from outside rustybuzz, and Rust's privacy
                // rules block `..` on a literal that can't name every
                // field, even though `Default` alone would fill them. Build
                // the default first, then set only the two public fields.
                let mut glyph_info = rustybuzz::GlyphInfo::default();
                glyph_info.glyph_id = glyph_id.0 as u32;
                glyph_info.cluster = x as u32;
                infos.push(glyph_info);
                positions.push(rustybuzz::GlyphPosition::default());
            }
            Some((infos, positions))
        };
        #[cfg(not(all(feature = "ascii_fast_shaping", not(feature = "bold_italic_fonts"))))]
        let fast_path: Option<(Vec<rustybuzz::GlyphInfo>, Vec<rustybuzz::GlyphPosition>)> = None;

        let (infos, positions): (Vec<rustybuzz::GlyphInfo>, Vec<rustybuzz::GlyphPosition>) =
            if let Some(fast) = fast_path {
                fast
            } else {
                // Shape the row
                let mut buffer = std::mem::take(&mut self.buffer);
                buffer.clear();
                for (idx, ch) in self.row.char_indices() {
                    buffer.add(ch, idx as u32);
                }

                let glyph_buffer =
                    shape_with_plan(font.font(), shared.plan_cache.get(font, &mut buffer), buffer);
                let infos = glyph_buffer.glyph_infos().to_vec();
                let positions = glyph_buffer.glyph_positions().to_vec();
                self.buffer = glyph_buffer.clear();
                (infos, positions)
            };

        // Process shaped (or synthesized) glyphs
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

            let cached = shared
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
                        shared.pending_cache_updates.push((*cached, bitmap));
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

                    shared.pending_cache_updates.push((rect, image));
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

            // Render at actual glyph width (no compression)
            let render_width_px = glyph_width_px as f32;

            // Background vertices - skipped when this cell's background
            // exactly matches the render pass's own clear color (see
            // `initial_fill_u32` above); the fg (text) quad below is
            // unaffected, so bg/fg quad counts intentionally diverge here.
            if bg_color_u32 != initial_fill_u32 {
                bg_vertices.push(TextBgVertexMember {
                    vertex: [screen_x, screen_y],
                    bg_color: bg_color_u32,
                });
                bg_vertices.push(TextBgVertexMember {
                    vertex: [screen_x + render_width_px, screen_y],
                    bg_color: bg_color_u32,
                });
                bg_vertices.push(TextBgVertexMember {
                    vertex: [screen_x, screen_y + self.fonts.height_px() as f32],
                    bg_color: bg_color_u32,
                });
                bg_vertices.push(TextBgVertexMember {
                    vertex: [
                        screen_x + render_width_px,
                        screen_y + self.fonts.height_px() as f32,
                    ],
                    bg_color: bg_color_u32,
                });
            }

            // Text vertices - 1:1 mapping between atlas and screen
            let uv_x = cached.x as f32;
            let uv_y = cached.y as f32;
            let uv_w = cached.width as f32; // Matches glyph_width_px
            let uv_h = cached.height as f32;

            text_vertices.push(TextVertexMember {
                vertex: [screen_x, screen_y],
                uv: [uv_x, uv_y],
                fg_color: fg_color_u32,
                underline_pos: 0,
                underline_color: fg_color_u32,
            });
            text_vertices.push(TextVertexMember {
                vertex: [screen_x + render_width_px, screen_y],
                uv: [uv_x + uv_w, uv_y],
                fg_color: fg_color_u32,
                underline_pos: 0,
                underline_color: fg_color_u32,
            });
            text_vertices.push(TextVertexMember {
                vertex: [screen_x, screen_y + self.fonts.height_px() as f32],
                uv: [uv_x, uv_y + uv_h],
                fg_color: fg_color_u32,
                underline_pos: 0,
                underline_color: fg_color_u32,
            });
            text_vertices.push(TextVertexMember {
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

        (bg_vertices, text_vertices)
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
    /// Two-pass, row-incremental design (IMPROVEMENT.md A2):
    /// 1. Regenerate every row `dirty_rows` says actually changed, via
    ///    `shape_row`, storing each row's geometry into
    ///    `row_geometry[y]` stamped with the atlas generation as of that
    ///    row's own processing.
    /// 2. For every remaining (clean) row, reuse its cached geometry
    ///    verbatim - UNLESS shaping this frame's dirty rows evicted
    ///    anything from the glyph atlas (`Atlas::generation()` advanced
    ///    since this flush started), in which case a cached row's UVs
    ///    might now point at a reassigned slot, so it is reshaped too,
    ///    this once. This is a graceful-degradation path: it only engages
    ///    under atlas pressure, at the same per-frame cost the pre-A2 code
    ///    always paid every frame.
    ///
    /// A cached row is trustworthy only while nothing has reassigned an
    /// atlas slot since it was recorded: `Atlas::get`'s eviction branch
    /// bumps `Atlas`'s generation counter, and `evictor::Lru::get` marks
    /// an entry "recently used" on every cache hit - so any row processed
    /// THIS flush (dirty, or reshaped here in pass 2) has its glyphs
    /// protected from eviction by later rows in the same flush; a row
    /// that stays cache-hit throughout is the one at risk, which is
    /// exactly what the generation check guards against.
    ///
    /// No index buffer is generated here - quad indices are a pure function
    /// of vertex count, derived in the render world (`TerminalGpuState` in
    /// `backend/mod.rs`) from a static, grow-only index buffer instead.
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
    /// cell). Nothing downstream reads this backend's vertex buffers
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

        // Clear the vertex buffers - they are full-frame state, rebuilt
        // from `row_geometry` (reused rows) and fresh shaping (regenerated
        // rows) below. `pending_cache_updates` must NOT be cleared here:
        // its entries are one-shot atlas uploads for slots the `Atlas` LRU
        // already considers cached, queued by a *previous* flush (or by
        // `populate_programmatic_glyphs` at creation) and not yet drained
        // by `take_draw_payload`. Clearing them here destroys the only
        // copy of those glyphs' pixels - the glyph is never re-rasterized
        // (the LRU says it's cached) and its slot stays garbage forever,
        // i.e. permanently invisible characters. This is exactly what
        // happens when `Terminal::draw` runs more than once between
        // payload-takes: creation-time populate + a setup-time initial
        // draw + the first per-frame draw are three flushes before the
        // first `gpu_flush_system` ever runs.
        self.bg_vertices.clear();
        self.text_vertices.clear();

        // Snapshot which rows ratatui's diff actually marked dirty coming
        // INTO this flush - `dirty_rows[y]` is cleared as each such row is
        // processed below, so this stable copy is what tells pass 2 which
        // rows still need looking at (a row just cleared by pass 1 must
        // not be reconsidered by pass 2 as "was already clean").
        let was_dirty = self.dirty_rows[..height].to_vec();

        // Cloned (not borrowed) so the closure below can hold it alongside
        // an independent `&mut self` for `self.shape_row(...)` - `self.fonts`
        // itself can't be borrowed for the `with_shared_cpu_state` receiver
        // while a closure captures all of `self` mutably for the `shape_row`
        // calls (IMPROVEMENT.md C3: the atlas/plan cache live in `Fonts`,
        // shared by every terminal using the same `Arc<Fonts>`).
        let fonts = Arc::clone(&self.fonts);
        fonts.with_shared_cpu_state(|shared| {
            let generation_before_flush = shared.cached.generation();

            // Pass 1: regenerate every row that actually changed.
            for (y, &dirty) in was_dirty.iter().enumerate() {
                if !dirty {
                    continue;
                }
                let (bg_vertices, text_vertices) = self.shape_row(y, width, shared);
                self.bg_vertices.extend_from_slice(&bg_vertices);
                self.text_vertices.extend_from_slice(&text_vertices);
                self.row_geometry[y] = RowGeometry {
                    bg_vertices,
                    text_vertices,
                    atlas_generation: shared.cached.generation(),
                };
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
                    self.bg_vertices
                        .extend_from_slice(&self.row_geometry[y].bg_vertices);
                    self.text_vertices
                        .extend_from_slice(&self.row_geometry[y].text_vertices);
                    continue;
                }

                let (bg_vertices, text_vertices) = self.shape_row(y, width, shared);
                self.bg_vertices.extend_from_slice(&bg_vertices);
                self.text_vertices.extend_from_slice(&text_vertices);
                self.row_geometry[y] = RowGeometry {
                    bg_vertices,
                    text_vertices,
                    atlas_generation: shared.cached.generation(),
                };
            }
        });

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
            // A `reset_bg` distinct from the default `initial_fill` (opaque
            // black) - otherwise this cell's resolved background exactly
            // matches the render pass's own clear color and B3's redundant-
            // quad skip (IMPROVEMENT.md B3) correctly omits it entirely,
            // which would starve this test of the vertices it inspects.
            .with_reset_bg([10, 20, 30])
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

    // ========================================================================
    // Test: A1 - flush() early-out on an unchanged frame (IMPROVEMENT.md A1).
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
        // real work and produce vertices.
        RatatuiBackend::draw(&mut backend, [(0u16, 0u16, &cell)].into_iter())
            .expect("draw failed");
        RatatuiBackend::flush(&mut backend).expect("flush failed");
        assert!(
            !backend.bg_vertices.is_empty(),
            "first draw onto an empty buffer must generate vertices"
        );

        // Drain the payload the way `Tui::flush` does, so the buffers are
        // empty going into the identical redraw below - otherwise "empty
        // after flush" would trivially pass even if flush regenerated the
        // same content.
        let _ = backend.take_draw_payload();
        assert!(backend.bg_vertices.is_empty() && backend.text_vertices.is_empty());

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

        assert!(
            backend.bg_vertices.is_empty(),
            "flush must skip regenerating bg vertices for an unchanged frame"
        );
        assert!(
            backend.text_vertices.is_empty(),
            "flush must skip regenerating text vertices for an unchanged frame"
        );
    }

    // ========================================================================
    // Test: A2 - row-incremental flush (IMPROVEMENT.md A2). Vertex order
    // is not part of the contract (two-pass processing visits rows in a
    // different order than a straight 0..height scan - see the doc
    // comment on `flush`), so vertices are compared as an order-independent
    // multiset via their raw bytes (`TextBgVertexMember`/`TextVertexMember`
    // are `bytemuck::Pod`, so this is a byte-exact comparison, not a
    // fuzzy one).
    // ========================================================================

    fn sorted_bytes<T: bytemuck::Pod>(items: &[T]) -> Vec<Vec<u8>> {
        let mut bytes: Vec<Vec<u8>> = items.iter().map(|i| bytemuck::bytes_of(i).to_vec()).collect();
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

        let incremental_bg = backend.bg_vertices.clone();
        let incremental_text = backend.text_vertices.clone();
        let _ = backend.take_draw_payload();

        // Force every row to be reshaped from scratch, on this same
        // backend. "A", "B", "Z", "D" are all already in the atlas from
        // the two flushes above, so this introduces no new glyphs and
        // therefore no new allocations - `Atlas::get` hits the existing
        // LRU entries and returns the SAME UV rects it already handed out,
        // making this a true apples-to-apples comparison against the
        // incremental flush above.
        backend.dirty_rows.iter_mut().for_each(|dirty| *dirty = true);
        backend.cells_changed_last_draw = true;
        RatatuiBackend::flush(&mut backend).expect("flush failed");

        assert_eq!(
            sorted_bytes(&incremental_bg),
            sorted_bytes(&backend.bg_vertices),
            "bg vertices from an incremental (row-cached) flush must match a \
             full reshape of the same content on the same atlas state"
        );
        assert_eq!(
            sorted_bytes(&incremental_text),
            sorted_bytes(&backend.text_vertices),
            "text vertices from an incremental (row-cached) flush must match a \
             full reshape of the same content on the same atlas state"
        );
    }
}
