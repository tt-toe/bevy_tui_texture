//! Terminal texture creation and ECS spawning.
//!
//! Two levels of abstraction for creating and managing terminals:
//!
//! 1. **`TerminalTexture`** - Core texture operations only
//!    - Just creates the terminal texture and ratatui Terminal
//!    - User must manually spawn entities and add input components
//!    - Maximum flexibility and control; wrap it in a [`Tui`] to draw with
//!      zero render-resource parameters per frame (`gpu_flush_system`,
//!      registered by `TerminalPlugin`, extracts a draw payload; the render
//!      world renders it directly into the terminal's `Image`/`GpuImage` -
//!      no CPU readback, no material touch needed)
//!
//! 2. **[`TuiRequest`]** - declarative spawning: spawn the request component
//!    (plus any `Node`/`Transform`/markers) and the plugin's
//!    `materialize_tui_requests` system creates the texture and inserts
//!    the right components (`Tui`, `ImageNode`+`Node` or
//!    `Mesh3d`+`MeshMaterial3d<StandardMaterial>`, `TerminalInput`) next
//!    frame - **no render resources in user code at all**
//!
//! For attaching a `Tui` to an *existing* mesh (e.g. a glTF primitive) or a
//! custom material type, see [`AttachTerminal`]/[`AttachMaterial`]
//! (combined with a [`TuiKind::Headless`] request for the `Tui` itself).
//!
//! # Examples
//!
//! ## Level 1: TerminalTexture (Manual Entity Management)
//!
//! ```ignore
//! use bevy::prelude::*;
//! use bevy_tui_texture::setup::TerminalTexture;
//! use bevy_tui_texture::prelude::*;
//!
//! fn setup(
//!     mut commands: Commands,
//!     mut images: ResMut<Assets<Image>>,
//! ) {
//!     let fonts = /* load fonts */;
//!
//!     let texture = TerminalTexture::create(
//!         80, 25, fonts, true, false, [0, 0, 0, 255], &mut images,
//!     ).unwrap();
//!
//!     // Manually spawn entity with input support
//!     commands.spawn((
//!         ImageNode { image: texture.image_handle(), ..default() },
//!         Node::default(),
//!         TerminalInput::default(),
//!         Tui::from_texture_state(texture),
//!         texture.dimensions(),
//!     ));
//! }
//! ```
//!
//! ## Level 2: TuiRequest (Declarative, 2D UI Overlay)
//!
//! ```ignore
//! use bevy::prelude::*;
//! use bevy_tui_texture::prelude::*;
//!
//! fn setup(mut commands: Commands) {
//!     commands.spawn((TuiRequest::ui(80, 25, fonts), Node::default()));
//! }
//! ```

use std::sync::Arc;
#[cfg(feature = "3d")]
use std::sync::Mutex;

#[cfg(feature = "3d")]
use bevy::pbr::{Material, StandardMaterial};
use bevy::prelude::*;

use crate::backend::bevy_backend::{BevyTerminalBackend, TerminalBuilder};
use crate::bevy_plugin::TerminalDimensions;
use crate::fonts::Fonts;
#[cfg(any(feature = "2d", feature = "3d"))]
use crate::input::TerminalInput;

/// Core terminal texture state without entity management.
///
/// This is the lowest-level abstraction - it creates the terminal texture and
/// ratatui Terminal, but does not spawn any entities. Users must manually:
/// - Spawn their own entity with the image
/// - Add input components if needed (TerminalInput, TerminalComponent, TerminalDimensions)
///
/// This provides maximum flexibility for users who want full control over
/// entity composition and component setup.
pub struct TerminalTexture {
    pub terminal: ratatui::Terminal<BevyTerminalBackend>,
    pub image_handle: Handle<Image>,
    pub width: u32,
    pub height: u32,
    cols: u16,
    rows: u16,
    char_width_px: u32,
    char_height_px: u32,
}

impl TerminalTexture {
    /// Create a new terminal texture without spawning an entity.
    ///
    /// # Arguments
    ///
    /// * `cols` - Number of columns (characters wide)
    /// * `rows` - Number of rows (characters tall)
    /// * `fonts` - Font configuration (shared via Arc)
    /// * `programmatic_glyphs` - If true, pre-populate box drawing, braille, and powerline glyphs
    /// * `transparent_reset_bg` - If true, cells with no explicit background
    ///   render with alpha 0 instead of an opaque fill - see
    ///   `TerminalConfig::transparent_reset_bg`.
    /// * `initial_fill` - Color shown before any content is drawn - see
    ///   `TerminalConfig::initial_fill`.
    /// * `images` - Bevy's Image assets
    ///
    /// # Returns
    ///
    /// Returns `Ok(TerminalTexture)` on success, or an error message on failure.
    ///
    /// # Example
    ///
    /// ```ignore
    /// # use bevy::prelude::*;
    /// # use bevy_tui_texture::setup::TerminalTexture;
    /// # fn setup(mut images: ResMut<Assets<Image>>) {
    /// let fonts = /* load fonts */;
    /// let texture = TerminalTexture::create(
    ///     80, 25, fonts, true, false, [0, 0, 0, 255], &mut images,
    /// ).unwrap();
    /// # }
    /// ```
    pub fn create(
        cols: u16,
        rows: u16,
        fonts: Arc<Fonts>,
        programmatic_glyphs: bool,
        transparent_reset_bg: bool,
        initial_fill: [u8; 4],
        images: &mut Assets<Image>,
    ) -> Result<Self, crate::TerminalError> {
        let char_width_px = fonts.min_width_px();
        let char_height_px = fonts.height_px();
        let width = cols as u32 * char_width_px;
        let height = rows as u32 * char_height_px;

        // Render-world-only image: no CPU-side pixel data ever exists for
        // it, and no main-world wgpu texture exists either - the render
        // world's `TerminalGpuState::render` (`backend/mod.rs`) renders
        // directly into this asset's own `GpuImage::texture_view` every
        // dirty frame (see `render_tui_textures` in `bevy_plugin.rs`).
        // `RENDER_ATTACHMENT` makes that possible; `TEXTURE_BINDING` lets
        // materials sample it; `COPY_SRC` backs `Tui::read_back_blocking`.
        let mut image = Image::new_uninit(
            bevy::render::render_resource::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            bevy::render::render_resource::TextureFormat::Rgba8Unorm,
            bevy::asset::RenderAssetUsages::RENDER_WORLD,
        );
        image.texture_descriptor.usage = bevy::render::render_resource::TextureUsages::RENDER_ATTACHMENT
            | bevy::render::render_resource::TextureUsages::TEXTURE_BINDING
            | bevy::render::render_resource::TextureUsages::COPY_SRC;
        let image_handle = images.add(image);

        // Create backend - pure CPU construction, no Device/Queue needed.
        let mut backend = TerminalBuilder::new(fonts)
            .with_dimensions(cols, rows)
            .with_transparent_reset_bg(transparent_reset_bg)
            .with_initial_fill(initial_fill)
            .build();

        // Optionally pre-populate programmatic glyphs
        if programmatic_glyphs {
            backend.populate_programmatic_glyphs();
        }

        let terminal = ratatui::Terminal::new(backend)?;

        Ok(Self {
            terminal,
            image_handle,
            width,
            height,
            cols,
            rows,
            char_width_px,
            char_height_px,
        })
    }

    /// Get the terminal dimensions for entity setup.
    ///
    /// Returns a `TerminalDimensions` component that should be added to
    /// the entity for input coordinate mapping to work correctly.
    pub fn dimensions(&self) -> TerminalDimensions {
        TerminalDimensions {
            cols: self.cols,
            rows: self.rows,
            char_width_px: self.char_width_px,
            char_height_px: self.char_height_px,
        }
    }

    /// Get the image handle for entity setup.
    ///
    /// This is the `Handle<Image>` that should be used in the entity's
    /// ImageNode or Material component.
    pub fn image_handle(&self) -> Handle<Image> {
        self.image_handle.clone()
    }

    /// Resize to a new grid size in place. Recreates the destination
    /// `Image` at the **same handle** (`images.insert`, not a new
    /// `images.add`) so every `ImageNode`/material already pointing at it
    /// keeps working with no re-pointing needed. Font metrics are
    /// unchanged by a grid resize, so pixel dimensions follow directly from
    /// the stored `char_width_px`/`char_height_px`.
    ///
    /// Does not itself touch ratatui's buffers - `Tui::apply_pending_resize`
    /// (the only caller) also resizes the backend and calls
    /// `Terminal::resize` immediately after this returns, so the two stay
    /// in lockstep.
    fn resize(&mut self, cols: u16, rows: u16, images: &mut Assets<Image>) {
        self.width = cols as u32 * self.char_width_px;
        self.height = rows as u32 * self.char_height_px;
        self.cols = cols;
        self.rows = rows;

        let mut image = Image::new_uninit(
            bevy::render::render_resource::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            bevy::render::render_resource::TextureFormat::Rgba8Unorm,
            bevy::asset::RenderAssetUsages::RENDER_WORLD,
        );
        image.texture_descriptor.usage = bevy::render::render_resource::TextureUsages::RENDER_ATTACHMENT
            | bevy::render::render_resource::TextureUsages::TEXTURE_BINDING
            | bevy::render::render_resource::TextureUsages::COPY_SRC;
        images
            .insert(&self.image_handle, image)
            .expect("resize: destination image handle must still be valid");
    }
}

/// Registry mapping click regions to caller-defined `u64` ids, rebuilt on
/// every [`Tui::draw_with_hits`] call. This is deliberately **a registry, not
/// a retained-mode UI** - ratatui has no widget tree, so there is no way to
/// derive hit regions automatically; record `(id, Rect)` next to the
/// `render_widget` call that draws it, then look it up at event time via
/// [`HitRegions::hit_at`].
///
/// `HitId` is a plain `u64` - no boxing, no dynamic dispatch, no
/// downcasting; chosen for speed over a boxed/dynamic alternative. The cost
/// is that callers encode their own hit-id enum into `u64` by hand
/// (mechanical, but cheap and `Copy`).
#[derive(Default)]
pub struct HitRegions {
    regions: Vec<(u64, ratatui::layout::Rect)>,
}

impl HitRegions {
    fn clear(&mut self) {
        self.regions.clear();
    }

    /// Register a hit region. Later registrations take priority over
    /// earlier, overlapping ones in [`HitRegions::hit_at`] (last-registered
    /// = topmost, matching draw order).
    pub fn add(&mut self, id: impl Into<u64>, rect: ratatui::layout::Rect) {
        self.regions.push((id.into(), rect));
    }

    /// Registers `block.inner(rect)` - the common case for bordered
    /// widgets, where the clickable area excludes the border.
    pub fn add_inner(
        &mut self,
        id: impl Into<u64>,
        block: &ratatui::widgets::Block,
        rect: ratatui::layout::Rect,
    ) {
        self.add(id, block.inner(rect));
    }

    /// Decodes the topmost (last-registered) region containing `pos`. If
    /// that region's id fails `T::try_from`, returns `None` - it does NOT
    /// fall through to regions underneath. (Decode failure means the caller
    /// used the wrong id type, not "try the next region"; falling through
    /// would silently mask that bug.)
    pub fn hit_at<T: TryFrom<u64>>(&self, pos: (u16, u16)) -> Option<T> {
        let point = ratatui::layout::Position {
            x: pos.0,
            y: pos.1,
        };
        self.regions
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(point))
            .and_then(|(id, _)| T::try_from(*id).ok())
    }
}

/// One terminal = one Entity. wgpu resources are `Send + Sync`, so this is a
/// plain Component (no `NonSend` needed).
///
/// Named `Tui`, not `Terminal` or `TuiTerminal`: a bare `Terminal` would
/// collide with `ratatui::prelude::Terminal` under the glob-import
/// combination every example already uses, and `TuiTerminal` spells out to
/// the redundant "Terminal User Interface Terminal".
///
/// [`Tui::draw`] only draws into ratatui's internal buffer and marks the
/// component dirty (skipped entirely if the draw was byte-identical to the
/// previous frame) - it touches no GPU state and is cheap to call every
/// frame. The actual GPU render is owned by the plugin's
/// [`gpu_flush_system`](crate::bevy_plugin::gpu_flush_system), registered
/// automatically in `TerminalSystemSet::Render`; a render-world system then
/// copies the result into this `Tui`'s `Image`/`GpuImage` - no CPU
/// readback, no material touch
#[derive(Component)]
pub struct Tui {
    texture_state: TerminalTexture,
    /// Starts `true` (not `false`): the uninitialized-texture guard. The
    /// destination `GpuImage` begins with genuinely undefined GPU memory
    /// (`Image::new_uninit`, no CPU data ever uploaded), so the first
    /// `gpu_flush_system` pass must run even if the caller hasn't drawn
    /// anything yet - `TerminalGpuState::render` clears it to black instead
    /// of leaving whatever garbage the GPU allocator handed back visible on
    /// screen.
    dirty: bool,
    /// Set by `flush` whenever it renders a fresh frame; drained by the
    /// render-world extract system (`extract_tui_draws` in
    /// `bevy_plugin.rs`), which is the only other reader/writer of this
    /// field.
    pending_draw: Option<crate::backend::TerminalDrawPayload>,
    /// Set by [`Tui::request_resize`]; applied by
    /// [`gpu_flush_system`](crate::bevy_plugin::gpu_flush_system) before
    /// `flush`, via [`Tui::apply_pending_resize`].
    pending_resize: Option<(u16, u16)>,
    hit_regions: HitRegions,
    /// Set once a `draw()`/`draw_with_hits()` call has logged its ratatui
    /// error, so a terminal that keeps failing every frame doesn't spam the
    /// log - one `warn!` per terminal is enough to diagnose it.
    draw_error_logged: bool,
}

impl Tui {
    /// Wrap an already-created [`TerminalTexture`]. This is the
    /// manual-entity-management constructor; for ergonomic spawning see the
    /// declarative [`TuiRequest`] component instead.
    pub fn from_texture_state(texture_state: TerminalTexture) -> Self {
        Self {
            texture_state,
            dirty: true,
            pending_draw: None,
            pending_resize: None,
            hit_regions: HitRegions::default(),
            draw_error_logged: false,
        }
    }

    /// Request a new grid size. No GPU work happens at the call site - it's
    /// applied on the next [`gpu_flush_system`](crate::bevy_plugin::gpu_flush_system)
    /// pass (before that frame's flush), which recreates the destination
    /// `Image` **in place at the same handle** (no `ImageNode`/material
    /// re-pointing needed) and resizes ratatui's own buffers, forcing a
    /// full redraw into the new texture. A no-op if `(cols, rows)` already
    /// matches the current grid size.
    ///
    /// No auto-fit helper ships:
    /// compute `cols`/`rows` yourself, typically from a `TerminalEventType::
    /// Resize` event's pixel size and `Tui::size_px()`'s per-cell metrics
    /// (see `examples/helloworld.rs` for the recipe).
    pub fn request_resize(&mut self, cols: u16, rows: u16) {
        let (current_cols, current_rows) = self.grid_size();
        if (cols, rows) != (current_cols, current_rows) {
            self.pending_resize = Some((cols, rows));
        }
    }

    /// Apply a pending resize, if any: recreate the destination `Image` at
    /// the new pixel size, update the backend's grid dimensions, and resize
    /// ratatui's own buffers immediately (rather than waiting for the next
    /// `Terminal::draw`'s `autoresize` to notice the mismatch) so the very
    /// next `Tui::draw` call already sees the new grid. Returns the new
    /// `(cols, rows)` if a resize was actually applied, for callers (the
    /// plugin's `gpu_flush_system`) that also need to sync a sibling
    /// `TerminalDimensions` component or recompute a `TuiKind::WorldQuad`
    /// mesh - both are keyed off this same return value via bevy's
    /// `Changed<T>` filters, not a separate signal.
    pub(crate) fn apply_pending_resize(&mut self, images: &mut Assets<Image>) -> Option<(u16, u16)> {
        let (cols, rows) = self.pending_resize.take()?;
        self.texture_state.resize(cols, rows, images);
        let backend = self.texture_state.terminal.backend_mut();
        backend.resize(cols, rows);

        // Drain whatever the backend currently holds (vertex geometry from
        // the last draw at the OLD grid size) - rendering that geometry
        // against the freshly resized texture would show garbled,
        // wrongly-scaled content for one frame, so it's discarded. Glyph
        // rasterizations queue in the shared per-font state (`Fonts`,
        // IMPROVEMENT.md C3), not in this payload, so there is nothing
        // grid-size-dependent left to preserve there. Any previously
        // unflushed payload (`self.pending_draw`, not yet drained by
        // `extract_tui_draws`) is simply replaced - its vertex data was
        // for an even older frame, equally stale.
        let mut payload = backend.take_draw_payload();
        payload.discard_stale_geometry();
        self.pending_draw = Some(payload);

        self.texture_state
            .terminal
            .resize(ratatui::layout::Rect::new(0, 0, cols, rows))
            .ok();
        // Already extracted a (correctly-sized, geometry-cleared) payload
        // above - `flush`, called right after this by `gpu_flush_system`,
        // must not extract a second one on top of it.
        self.dirty = false;
        Some((cols, rows))
    }

    /// Draw with ratatui. Touches no GPU state - renders into the backend
    /// buffer and, if the ratatui diff actually changed at least one cell,
    /// sets the dirty flag (a byte-identical redraw is a no-op past this
    /// point: no GPU render, no copy, next frame). Cheap to call every
    /// frame either way.
    ///
    /// Error handling: ratatui's `Terminal::draw` returns `io::Result`; this
    /// crate cannot recover from a backend draw failure, so it does not
    /// panic - but the first occurrence per `Tui` is logged via `warn!`
    /// (further occurrences are suppressed to avoid per-frame log spam).
    pub fn draw(&mut self, f: impl FnOnce(&mut ratatui::Frame)) {
        match self.texture_state.terminal.draw(f) {
            Ok(_) => self.mark_dirty_if_changed(),
            Err(err) => self.log_draw_error(err),
        }
    }

    /// `draw()` variant handing the caller a `&mut HitRegions` alongside the
    /// `Frame`, so click regions are registered right next to the
    /// `render_widget` call that draws them. Regions are cleared at the
    /// start of each call - register fresh ones every draw.
    pub fn draw_with_hits(&mut self, f: impl FnOnce(&mut ratatui::Frame, &mut HitRegions)) {
        self.hit_regions.clear();
        let hit_regions = &mut self.hit_regions;
        match self
            .texture_state
            .terminal
            .draw(|frame| f(frame, hit_regions))
        {
            Ok(_) => self.mark_dirty_if_changed(),
            Err(err) => self.log_draw_error(err),
        }
    }

    fn mark_dirty_if_changed(&mut self) {
        if self.texture_state.terminal.backend().cells_changed_last_draw() {
            self.dirty = true;
        }
    }

    fn log_draw_error(&mut self, err: std::io::Error) {
        if !self.draw_error_logged {
            tracing::warn!("Tui::draw failed, content for this terminal may be stale: {err}");
            self.draw_error_logged = true;
        }
    }

    /// The hit regions registered by the most recent [`Tui::draw_with_hits`]
    /// call.
    pub fn hit_regions(&self) -> &HitRegions {
        &self.hit_regions
    }

    /// Actual pixel size of the texture.
    pub fn size_px(&self) -> UVec2 {
        UVec2::new(self.texture_state.width, self.texture_state.height)
    }

    /// Terminal grid size (columns, rows).
    pub fn grid_size(&self) -> (u16, u16) {
        let d = self.texture_state.dimensions();
        (d.cols, d.rows)
    }

    /// The `Handle<Image>` this terminal renders into.
    pub fn image_handle(&self) -> &Handle<Image> {
        &self.texture_state.image_handle
    }

    /// Read this terminal's current pixels back to the CPU, **blocking**
    /// until the render world performs the copy. An explicit opt-in for
    /// screenshots and tests only - the normal per-frame path never touches
    /// the CPU at all; do not call this every frame. Returns tightly-packed
    /// RGBA8 bytes. Goes through the render world via a request/response
    /// channel (`TuiReadbackChannel` in `bevy_plugin.rs`) - there is no
    /// main-world texture to read from directly in Phase B.
    pub fn read_back_blocking(&self, channel: &crate::bevy_plugin::TuiReadbackChannel) -> Vec<u8> {
        channel.request_blocking(self.texture_state.image_handle.id())
    }

    /// Called by [`gpu_flush_system`](crate::bevy_plugin::gpu_flush_system).
    /// If dirty, extracts the CPU-computed draw payload from the backend and
    /// stashes it for the render-world extract system to pick up; the
    /// actual GPU render happens there, not here.
    pub(crate) fn flush(&mut self) {
        if self.dirty {
            let draw = self.texture_state.terminal.backend_mut().take_draw_payload();
            // Simply replaces any not-yet-extracted previous payload (the
            // render world skipped a frame - it extracts every frame once
            // running, but sits out the first few while the renderer
            // initializes asynchronously). That older payload's vertex
            // data was for an even earlier frame, equally stale; glyph
            // rasterizations live in the shared per-font state (`Fonts`,
            // IMPROVEMENT.md C3) rather than in this payload, so there is
            // nothing one-shot left here to lose by dropping it.
            self.pending_draw = Some(draw);
            self.dirty = false;
        }
    }

    /// Drain the pending draw payload, if set, returning it alongside the
    /// destination image's asset id. Called once per frame by the
    /// render-world extract system (`extract_tui_draws` in
    /// `bevy_plugin.rs`) via `ResMut<MainWorld>`.
    pub(crate) fn take_pending_draw(
        &mut self,
    ) -> Option<(AssetId<Image>, crate::backend::TerminalDrawPayload)> {
        self.pending_draw
            .take()
            .map(|draw| (self.texture_state.image_handle.id(), draw))
    }

    /// Returns this terminal's font identity key, alongside any glyph
    /// rasterizations still pending upload to that font's shared atlas
    /// (IMPROVEMENT.md C3). The upload list is frequently empty: whichever
    /// terminal sharing a given font is visited first in a frame drains it
    /// for all of them (see `Fonts::with_shared_cpu_state`). Called once
    /// per `Tui` per frame by the render-world extract system
    /// (`extract_tui_draws` in `bevy_plugin.rs`).
    pub(crate) fn take_shared_font_uploads(
        &self,
    ) -> (usize, Vec<(crate::utils::text_atlas::CacheRect, Vec<u32>)>) {
        let backend = self.texture_state.terminal.backend();
        (backend.font_key(), backend.take_shared_glyph_uploads())
    }
}

/// Marks an entity as the visible *surface* for a terminal, when the surface
/// and the [`Tui`] component live on different entities (the "attach to an
/// existing mesh" case, see [`AttachTerminal`]). For library-spawned
/// terminals the surface and the `Tui` are the same entity (`tui == self`).
///
/// The input system reads this to remap `TerminalEvent::target` from the
/// surface entity (where hit-testing happens, via `TerminalInput`/
/// `TerminalDimensions`) to the `Tui` entity, so user event code never needs
/// to know whether a terminal was library-spawned or attached.
#[derive(Component, Debug, Clone, Copy)]
pub struct TuiSurface {
    pub tui: Entity,
}

// ============================================================================
// Config struct + declarative spawning
// ============================================================================

/// Configuration for [`TuiRequest`]. A single struct instead of a long run
/// of positional bools, so call sites don't need to annotate every argument
/// with a comment to stay readable.
pub struct TerminalConfig {
    /// Pre-populate box-drawing, braille, and powerline glyphs.
    pub programmatic_glyphs: bool,
    /// Whether this terminal can receive keyboard input (focus required).
    pub keyboard: bool,
    /// Whether this terminal can receive mouse input.
    pub mouse: bool,
    /// Drawn once at creation time (before the entity's own draw system
    /// runs), so the very first presented frame already has real content
    /// instead of the create-time fill color. (`Sync` bound because this
    /// struct rides inside the `TuiRequest` component.)
    pub initial_draw: Option<Box<dyn FnOnce(&mut ratatui::Frame) + Send + Sync>>,
    /// Color shown before any content has been drawn (or, transiently,
    /// mid-resize - see `Tui::request_resize`). Default opaque black
    /// (`[0, 0, 0, 255]`).
    pub initial_fill: [u8; 4],
    /// If `true`, cells with no explicit background (`ratatui::style::
    /// Color::Reset`, ratatui's own default) render with alpha 0 instead
    /// of an opaque fill color - letting a `TuiKind::WorldQuad` terminal
    /// with `alpha_mode: AlphaMode::Blend` show the scene through its
    /// background, or a `TuiKind::Ui` terminal show through to whatever is
    /// behind its `Node`. Cells with an explicit background color (`Color::
    /// Rgb`/`Indexed`/etc., including any of ratatui's named colors) are
    /// unaffected - only `Reset` becomes transparent. Default `false`.
    pub transparent_reset_bg: bool,
    /// Alpha/transparency mode for the material [`TuiKind::WorldQuad`]
    /// builds. Default `AlphaMode::Opaque`; combine with
    /// `transparent_reset_bg: true` for a HUD-style see-through screen.
    /// Ignored by `TuiKind::Ui`/`Headless` (2D UI transparency is
    /// controlled by `transparent_reset_bg` alone - `ImageNode` always
    /// respects its texture's alpha).
    #[cfg(feature = "3d")]
    pub alpha_mode: AlphaMode,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            programmatic_glyphs: true,
            keyboard: true,
            mouse: true,
            initial_draw: None,
            initial_fill: [0, 0, 0, 255],
            transparent_reset_bg: false,
            #[cfg(feature = "3d")]
            alpha_mode: AlphaMode::Opaque,
        }
    }
}

/// Where a [`TuiRequest`]'s font comes from.
pub enum TuiFontSource {
    /// An already-constructed font set - the common native path
    /// (`include_bytes!` + [`Font::new`](crate::fonts::Font::new), or
    /// runtime bytes + [`Font::from_vec`](crate::fonts::Font::from_vec)).
    Ready(Arc<Fonts>),
    /// Load via the `AssetServer` (Wasm-safe - `std::fs::read` doesn't work
    /// on Wasm). The request stays pending until the asset resolves; the
    /// terminal then materializes with the font rendered at `size_px`.
    Asset {
        handle: Handle<crate::fonts::TerminalFontAsset>,
        size_px: u32,
    },
}

impl From<Arc<Fonts>> for TuiFontSource {
    fn from(fonts: Arc<Fonts>) -> Self {
        TuiFontSource::Ready(fonts)
    }
}

/// What display surface `materialize_tui_requests` builds for a
/// [`TuiRequest`].
#[derive(Clone, Copy)]
pub enum TuiKind {
    /// A bevy_ui terminal: `TuiUi` + `ImageNode` (+ a required `Node`,
    /// yours if you spawned one on the entity, a default otherwise).
    #[cfg(feature = "2d")]
    Ui,
    /// A 3D quad sized in **world units**: `height` in world units, width
    /// follows the texture's pixel aspect ratio. The quad's visible face
    /// normal is local `+Z` - orient it with an ordinary `Transform` on the
    /// same entity, e.g. to face a camera:
    /// `Transform::from_translation(pos).with_rotation(Quat::from_rotation_arc(Vec3::Z, camera_pos - pos))`.
    /// (`Transform::looking_at` aligns local `-Z` with the target - the
    /// *opposite* convention - and would show the quad's back.)
    ///
    /// The `Mesh3d` + `MeshMaterial3d<StandardMaterial>` (unlit, textured
    /// with the terminal) are only inserted if the entity doesn't already
    /// carry them - but for a fully custom mesh or material type, prefer
    /// [`TuiKind::Headless`] plus your own surface entity (a custom
    /// `MeshMaterial3d<M>` is a *different component type*, so the
    /// `StandardMaterial` one would be inserted alongside it, not skipped).
    #[cfg(feature = "3d")]
    WorldQuad { height: f32 },
    /// A `Tui` with no surface components of its own - for terminals whose
    /// display surface is an existing mesh claimed via [`AttachTerminal`],
    /// or a fully custom entity setup around
    /// [`Tui::image_handle`](Tui::image_handle).
    Headless,
}

/// Records a [`TuiKind::WorldQuad`] terminal's configured world-unit height,
/// inserted at materialization. Read back by the plugin's resize handling
/// (`gpu_flush_system`) to recompute the mesh's aspect ratio when the grid
/// size (and thus the texture's pixel aspect ratio) changes.
#[cfg(feature = "3d")]
#[derive(Component, Clone, Copy)]
pub(crate) struct WorldQuadHeight(pub(crate) f32);

/// Declarative terminal request: spawn this component (plus any `Node` /
/// `Transform` / marker components you want on the terminal entity), and
/// the plugin's `materialize_tui_requests` system does the rest - **your
/// systems never touch `Assets<Image>` or any other render resource**.
///
/// ```ignore
/// commands.spawn((
///     TuiRequest::ui(80, 25, fonts),
///     Node {
///         position_type: PositionType::Absolute,
///         right: Val::Px(20.0),
///         top: Val::Px(20.0),
///         ..default()
///     },
///     MyTerminalMarker,
/// ));
/// ```
///
/// The request materializes on the next frame (or once the font asset
/// resolves, for [`TuiFontSource::Asset`]): `TuiRequest` is removed and the
/// same components the immediate-mode helpers used to build are inserted.
/// Query for `&mut Tui` with your marker as usual - just tolerate it not
/// existing yet (`let Ok(..) = query.single_mut() else { return }`), which
/// idiomatic bevy systems do anyway.
#[derive(Component)]
pub struct TuiRequest {
    pub cols: u16,
    pub rows: u16,
    pub fonts: TuiFontSource,
    pub kind: TuiKind,
    pub config: TerminalConfig,
}

impl TuiRequest {
    /// A bevy_ui terminal (see [`TuiKind::Ui`]) with default
    /// [`TerminalConfig`].
    #[cfg(feature = "2d")]
    pub fn ui(cols: u16, rows: u16, fonts: impl Into<TuiFontSource>) -> Self {
        Self {
            cols,
            rows,
            fonts: fonts.into(),
            kind: TuiKind::Ui,
            config: TerminalConfig::default(),
        }
    }

    /// A world-unit-sized 3D quad terminal (see [`TuiKind::WorldQuad`])
    /// with default [`TerminalConfig`].
    #[cfg(feature = "3d")]
    pub fn world_quad(cols: u16, rows: u16, fonts: impl Into<TuiFontSource>, height: f32) -> Self {
        Self {
            cols,
            rows,
            fonts: fonts.into(),
            kind: TuiKind::WorldQuad { height },
            config: TerminalConfig::default(),
        }
    }

    /// A surface-less terminal (see [`TuiKind::Headless`]) with default
    /// [`TerminalConfig`].
    pub fn headless(cols: u16, rows: u16, fonts: impl Into<TuiFontSource>) -> Self {
        Self {
            cols,
            rows,
            fonts: fonts.into(),
            kind: TuiKind::Headless,
            config: TerminalConfig::default(),
        }
    }

    /// Replace the default [`TerminalConfig`].
    pub fn with_config(mut self, config: TerminalConfig) -> Self {
        self.config = config;
        self
    }
}

/// Marker for [`TuiKind::Ui`] terminals. Requires `Node`: if the caller's
/// own spawn tuple includes a `Node`, that one wins (bevy only auto-inserts
/// a required component when the entity doesn't already have one);
/// otherwise a default `Node` is inserted so the entity is still a valid UI
/// node.
#[cfg(feature = "2d")]
#[derive(Component, Default)]
#[require(Node)]
pub struct TuiUi;

/// Plugin system backing [`TuiRequest`]. Registered automatically by
/// `TerminalPlugin`, scheduled before `TerminalSystemSet::Input` so a
/// terminal materialized this frame is visible to the same frame's input
/// and user-draw systems.
///
/// For each entity with `TuiRequest` and without `Tui`: resolve the font
/// ([`TuiFontSource::Asset`] requests stay pending until the asset loads -
/// or are dropped with a `warn!` if the load fails), create the
/// [`TerminalTexture`], insert the surface components for the request's
/// [`TuiKind`], and remove the `TuiRequest`. User-supplied components win:
/// surface components are inserted via `insert_if_new`, so a `Node`,
/// `Transform`, `TerminalInput`, or (for `WorldQuad`) `Mesh3d`/
/// `MeshMaterial3d<StandardMaterial>` already on the entity is kept.
pub(crate) fn materialize_tui_requests(
    mut commands: Commands,
    mut requests: Query<(Entity, &mut TuiRequest), Without<Tui>>,
    asset_server: Res<AssetServer>,
    font_assets: Res<Assets<crate::fonts::TerminalFontAsset>>,
    mut images: ResMut<Assets<Image>>,
    // `Option`: these assets only exist once something registers them
    // (bevy's PbrPlugin, normally) - a headless or UI-only app shouldn't
    // fail this system's parameter validation over resources that only
    // `TuiKind::WorldQuad` needs.
    #[cfg(feature = "3d")] mut meshes: Option<ResMut<Assets<Mesh>>>,
    #[cfg(feature = "3d")] mut materials: Option<ResMut<Assets<StandardMaterial>>>,
) {
    for (entity, mut request) in &mut requests {
        let fonts = match &request.fonts {
            TuiFontSource::Ready(fonts) => fonts.clone(),
            TuiFontSource::Asset { handle, size_px } => match font_assets.get(handle) {
                Some(asset) => match Fonts::from_asset(asset, *size_px) {
                    Ok(fonts) => fonts,
                    Err(err) => {
                        tracing::warn!("TuiRequest dropped: font asset failed to parse: {err}");
                        commands.entity(entity).remove::<TuiRequest>();
                        continue;
                    }
                },
                None => {
                    if asset_server.load_state(handle).is_failed() {
                        tracing::warn!("TuiRequest dropped: font asset failed to load");
                        commands.entity(entity).remove::<TuiRequest>();
                    }
                    continue; // still loading - retry next frame
                }
            },
        };

        let texture_state = match TerminalTexture::create(
            request.cols,
            request.rows,
            fonts,
            request.config.programmatic_glyphs,
            request.config.transparent_reset_bg,
            request.config.initial_fill,
            &mut images,
        ) {
            Ok(texture_state) => texture_state,
            Err(err) => {
                tracing::warn!("TuiRequest dropped: terminal creation failed: {err}");
                commands.entity(entity).remove::<TuiRequest>();
                continue;
            }
        };

        #[cfg(any(feature = "2d", feature = "3d"))]
        let dimensions = texture_state.dimensions();
        #[cfg(any(feature = "2d", feature = "3d"))]
        let image_handle = texture_state.image_handle();
        let mut tui = Tui::from_texture_state(texture_state);
        if let Some(initial_draw) = request.config.initial_draw.take() {
            tui.draw(initial_draw);
        }
        #[cfg(any(feature = "2d", feature = "3d"))]
        let input = TerminalInput {
            keyboard: request.config.keyboard,
            mouse: request.config.mouse,
        };

        let mut entity_commands = commands.entity(entity);
        entity_commands.remove::<TuiRequest>();
        match request.kind {
            #[cfg(feature = "2d")]
            TuiKind::Ui => {
                entity_commands.insert((tui, dimensions)).insert_if_new((
                    TuiUi,
                    ImageNode {
                        image: image_handle,
                        ..default()
                    },
                    input,
                ));
            }
            #[cfg(feature = "3d")]
            TuiKind::WorldQuad { height } => {
                let (Some(meshes), Some(materials)) = (meshes.as_mut(), materials.as_mut())
                else {
                    tracing::warn!(
                        "TuiRequest dropped: TuiKind::WorldQuad needs Assets<Mesh> and \
                         Assets<StandardMaterial> (registered by bevy's PbrPlugin)"
                    );
                    continue;
                };
                let aspect = dimensions.cols as f32 * dimensions.char_width_px as f32
                    / (dimensions.rows as f32 * dimensions.char_height_px as f32);
                let half_height = height / 2.0;
                let mesh = meshes.add(Plane3d::new(
                    Vec3::Z,
                    Vec2::new(half_height * aspect, half_height),
                ));
                let material = materials.add(StandardMaterial {
                    base_color: Color::WHITE,
                    base_color_texture: Some(image_handle),
                    // Terminal content should not depend on scene lighting.
                    unlit: true,
                    alpha_mode: request.config.alpha_mode,
                    double_sided: true,
                    cull_mode: None,
                    ..default()
                });
                entity_commands
                    .insert((tui, dimensions, WorldQuadHeight(height)))
                    .insert_if_new((Mesh3d(mesh), MeshMaterial3d(material), input));
            }
            TuiKind::Headless => {
                entity_commands.insert(tui);
            }
        }
    }
}

// ============================================================================
// Attaching a Tui to an existing mesh
// ============================================================================

/// Type-erased "insert this material" action. Never constructed directly -
/// [`AttachMaterial::standard`]/[`AttachMaterial::custom`] build it. `Arc`
/// (not `Box`) so `attach_terminal_system` can cheaply clone it out of a
/// query item into a `Commands::queue` closure every re-claim attempt.
#[cfg(feature = "3d")]
#[derive(Clone)]
struct UntypedMaterialInsert(std::sync::Arc<dyn Fn(Handle<Image>, Entity, &mut World) + Send + Sync>);

/// How to material a [`AttachTerminal`]-marked mesh. Fully type-erased so
/// `AttachTerminal` itself never needs a generic parameter - a generic
/// `AttachTerminal<M>` would force every call site, query, and system to
/// name `M`.
#[cfg(feature = "3d")]
pub struct AttachMaterial(UntypedMaterialInsert);

#[cfg(feature = "3d")]
impl AttachMaterial {
    /// Plain `StandardMaterial`, unlit, textured with the terminal.
    /// `alpha_mode: AlphaMode::Blend` combined with the attached `Tui`'s
    /// `TerminalConfig::transparent_reset_bg: true` shows the scene through
    /// cells with no explicit background.
    pub fn standard(alpha_mode: AlphaMode) -> Self {
        Self::custom(move |image| StandardMaterial {
            base_color_texture: Some(image),
            unlit: true,
            alpha_mode,
            ..default()
        })
    }

    /// Any material type. `factory` builds a concrete material `M` from the
    /// terminal's image handle - no plugin registration needed for any `M`,
    /// custom or `StandardMaterial`: the render-world render
    /// (`render_tui_textures` in `bevy_plugin.rs`) writes directly into the
    /// same GPU texture the material's bind group already references.
    ///
    /// `factory` is invoked at most once per entity even though
    /// `attach_terminal_system` may call this action every frame while
    /// re-claiming (see that function's doc comment): the built handle is
    /// cached internally and reused on subsequent calls, so re-claiming
    /// never mints duplicate material assets.
    pub fn custom<M: Material>(
        factory: impl Fn(Handle<Image>) -> M + Send + Sync + 'static,
    ) -> Self {
        let cached: Mutex<Option<Handle<M>>> = Mutex::new(None);
        AttachMaterial(UntypedMaterialInsert(std::sync::Arc::new(
            move |image, entity, world| {
                let mut cached = cached.lock().unwrap();
                // Not `get_or_insert_with`: its closure would have to either
                // borrow `image` (forcing a clone to hand `factory` an owned
                // `Handle<Image>`) or `move` it, which would also move
                // `world` out of this scope, breaking the `entity_mut` call
                // below. Branching by hand lets the `None` arm move `image`
                // into `factory` directly, with no clone.
                let handle = match cached.as_ref() {
                    Some(handle) => handle.clone(),
                    None => {
                        let handle = world.resource_mut::<Assets<M>>().add(factory(image));
                        *cached = Some(handle.clone());
                        handle
                    }
                };
                // `TuiAttached` records exactly the handle just installed, so
                // `attach_terminal_system` can recognize its own claim next
                // frame and skip the remove+insert archetype churn entirely
                // (see that function's doc comment).
                world.entity_mut(entity).insert((
                    MeshMaterial3d(handle.clone()),
                    TuiAttached {
                        material: handle.untyped(),
                    },
                ));
            },
        )))
    }
}

/// Bookkeeping component: records the material handle
/// [`attach_terminal_system`] most recently installed on an
/// [`AttachTerminal`]-marked entity, so it can tell "still ours" (no-op)
/// apart from "the loader stomped us" (re-claim) without an archetype move
/// on every settled frame. See that function's doc comment for the full
/// picture.
#[cfg(feature = "3d")]
#[derive(Component)]
pub(crate) struct TuiAttached {
    material: UntypedHandle,
}

/// Insert on a mesh entity (e.g. a glTF primitive) to attach a `Tui` to it.
/// `attach_terminal_system` then:
/// 1. builds (or re-fetches the cached) material via [`AttachMaterial`],
///    handing it the terminal's `Handle<Image>`, and swaps out
///    `MeshMaterial3d<StandardMaterial>`,
/// 2. inserts `TuiSurface { tui }` + `TerminalInput` + `TerminalDimensions`
///    (mouse picking works on any UV-mapped mesh, curved included; the input
///    system remaps event targets through `TuiSurface`, so user event code
///    is identical to the library-spawned case),
/// 3. RE-CLAIMS only while the entity's current
///    `MeshMaterial3d<StandardMaterial>` handle differs from the
///    `TuiAttached` bookkeeping component's recorded handle (e.g. a glTF
///    loader asynchronously re-inserting its own stock material over ours -
///    see CLAUDE.md "Common Gotchas" #8) - once the installed handle is
///    recognized as already ours, the entity is skipped with **no**
///    archetype move at all, for [`AttachMaterial::custom`] targets of a
///    type other than `StandardMaterial` (which naturally drop out of the
///    query below once `MeshMaterial3d<StandardMaterial>` is gone) as well
///    as [`AttachMaterial::standard`] (which keeps re-matching the query
///    forever, but now settles into true no-ops).
#[cfg(feature = "3d")]
#[derive(Component)]
pub struct AttachTerminal {
    /// Entity carrying the `Tui` component to display on this mesh.
    pub terminal: Entity,
    pub material: AttachMaterial,
}

/// Plugin system backing [`AttachTerminal`]. Registered automatically by
/// `TerminalPlugin`. See `AttachTerminal`'s doc comment for the full
/// behavior.
#[cfg(feature = "3d")]
pub(crate) fn attach_terminal_system(
    mut commands: Commands,
    to_attach: Query<(
        Entity,
        &AttachTerminal,
        &MeshMaterial3d<StandardMaterial>,
        Option<&TuiAttached>,
    )>,
    terminals: Query<&Tui>,
) {
    for (surface_entity, attach, current_material, attached) in &to_attach {
        if attached.is_some_and(|a| a.material.id() == current_material.0.id().untyped()) {
            continue; // already ours - no archetype churn
        }

        let Ok(tui) = terminals.get(attach.terminal) else {
            continue; // Tui not spawned (yet) - try again next frame
        };

        let image = tui.image_handle().clone();
        let (cols, rows) = tui.grid_size();
        let size = tui.size_px();
        let char_width_px = size.x / (cols.max(1) as u32);
        let char_height_px = size.y / (rows.max(1) as u32);
        let tui_entity = attach.terminal;
        let insert = attach.material.0.clone();

        commands
            .entity(surface_entity)
            .remove::<MeshMaterial3d<StandardMaterial>>();
        commands.queue(move |world: &mut World| {
            (insert.0)(image, surface_entity, world);
        });
        commands.entity(surface_entity).insert((
            TuiSurface { tui: tui_entity },
            TerminalInput::default(),
            TerminalDimensions {
                cols,
                rows,
                char_width_px,
                char_height_px,
            },
        ));
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tui_flush_tests {
    use super::*;
    use crate::fonts::{Font, Fonts};
    use ratatui::style::{Color as RatatuiColor, Style};
    use ratatui::widgets::Block;

    fn test_fonts() -> Arc<Fonts> {
        let font_data = include_bytes!("../examples/assets/fonts/Mplus1Code-Regular.ttf");
        let font = Font::new(font_data).expect("failed to load test font");
        Arc::new(Fonts::new(font, 16))
    }

    /// A repeatedly-failing `draw()` logs its error only once per `Tui`,
    /// not once per call - `log_draw_error` is the de-duplication gate this
    /// asserts on directly (a real ratatui backend failure is impractical
    /// to trigger from the outside, so this drives the same private method
    /// `draw`/`draw_with_hits` call on error). Pure CPU - `TerminalTexture`
    /// needs no `RenderDevice`/`RenderQueue` since Phase B.
    #[test]
    fn draw_error_logged_only_once() {
        let mut images = Assets::<Image>::default();
        let texture_state = TerminalTexture::create(4, 2, test_fonts(), false, false, [0, 0, 0, 255], &mut images)
            .expect("failed to create terminal texture");
        let mut tui = Tui::from_texture_state(texture_state);

        assert!(!tui.draw_error_logged);
        let fake_err = || std::io::Error::other("simulated backend failure");
        tui.log_draw_error(fake_err());
        assert!(tui.draw_error_logged, "first error must flip the flag");
        // Second call must not panic and must leave the flag set - the
        // actual "only warn! once" behavior lives in the `if
        // !self.draw_error_logged` guard inside `log_draw_error` itself.
        tui.log_draw_error(fake_err());
        assert!(tui.draw_error_logged);
    }

    /// No-change skip (design point 7): redrawing byte-identical content
    /// must not re-mark the terminal dirty, so `flush` performs no work on
    /// the second call. Pure CPU - `draw()`'s dirty tracking is ratatui
    /// buffer-diff logic, unrelated to the render world.
    #[test]
    fn identical_redraw_does_not_mark_dirty() {
        let mut images = Assets::<Image>::default();
        let texture_state = TerminalTexture::create(4, 2, test_fonts(), false, false, [0, 0, 0, 255], &mut images)
            .expect("failed to create terminal texture");
        let mut tui = Tui::from_texture_state(texture_state);

        let paint_red = |frame: &mut ratatui::Frame| {
            frame.render_widget(
                Block::default().style(Style::default().bg(RatatuiColor::Red)),
                frame.area(),
            );
        };

        // First draw: empty -> red is definitely a change. (`dirty` starts
        // `true` from the uninitialized-texture guard - draw+flush once to
        // get to a clean baseline before testing the no-change skip.)
        tui.draw(paint_red);
        assert!(tui.dirty, "first draw onto an empty buffer must be dirty");
        tui.flush();
        assert!(!tui.dirty, "flush must clear dirty");

        // Second draw: byte-identical content -> zero-cell diff -> must
        // NOT re-mark dirty (this is the whole point of the optimization).
        tui.draw(paint_red);
        assert!(
            !tui.dirty,
            "identical redraw must not mark dirty (no-change skip)"
        );
    }

    /// Regression test: draw once inside a real headless bevy render
    /// world, then read the destination `Image` back via
    /// [`Tui::read_back_blocking`]. Exercises the full Phase B pipeline -
    /// `gpu_flush_system` -> `extract_tui_draws` -> `render_tui_textures` ->
    /// `TerminalGpuState::render` -> `process_tui_readbacks` - end to end.
    ///
    /// `read_back_blocking` is a genuine blocking round-trip through the
    /// render world's channel, so it must be called from a different thread
    /// than the one driving `app.update()` - exactly like a real
    /// application using `PipelinedRenderingPlugin`. Skips (rather than
    /// fails) if no GPU adapter shows up within the bound, matching the
    /// deferred plan to not make CI depend on GPU availability.
    #[test]
    fn flush_renders_drawn_content_synchronously() {
        use crate::bevy_plugin::TuiReadbackChannel;
        use crate::bevy_plugin::TerminalPlugin;
        use bevy::render::renderer::RenderDevice;

        let mut app = App::new();
        app.add_plugins((
            bevy::app::TaskPoolPlugin::default(),
            bevy::asset::AssetPlugin::default(),
            // `primary_window: None` - headless, no actual OS window - but
            // still registers the window message types (`WindowResized` and
            // friends) that both `window_resize_system` (always registered
            // by `TerminalPlugin`) and bevy_render's own window-extraction
            // systems read.
            bevy::window::WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            },
            bevy::mesh::MeshPlugin,
            bevy::diagnostic::FrameCountPlugin,
            bevy::time::TimePlugin,
            bevy::render::RenderPlugin::default(),
            bevy::image::ImagePlugin::default(),
            TerminalPlugin::display_only(),
        ));
        // Ordinarily inserted by `bevy_camera::CameraPlugin` (part of
        // `DefaultPlugins`, which this headless test intentionally doesn't
        // pull in) - `bevy_render`'s own camera module only extracts it,
        // it doesn't create it.
        app.init_resource::<bevy::camera::ClearColor>();
        app.finish();
        app.cleanup();

        let mut ready = false;
        for _ in 0..200 {
            app.update();
            if app.world().get_resource::<RenderDevice>().is_some() {
                ready = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if !ready {
            eprintln!("skipping: no GPU adapter available in this environment");
            return;
        }

        let entity = {
            let mut images = app.world_mut().resource_mut::<Assets<Image>>();
            let texture_state = TerminalTexture::create(4, 2, test_fonts(), false, false, [0, 0, 0, 255], &mut images)
                .expect("failed to create terminal texture");
            let mut tui = Tui::from_texture_state(texture_state);
            tui.draw(|frame| {
                frame.render_widget(
                    Block::default().style(Style::default().bg(RatatuiColor::Red)),
                    frame.area(),
                );
            });
            app.world_mut().spawn(tui).id()
        };

        let image_id = app
            .world()
            .get::<Tui>(entity)
            .unwrap()
            .image_handle()
            .id();
        let channel = app.world().resource::<TuiReadbackChannel>().clone();
        let readback = std::thread::spawn(move || channel.request_blocking(image_id));

        let mut pixels = None;
        for _ in 0..200 {
            app.update();
            if readback.is_finished() {
                pixels = Some(readback.join().expect("readback thread panicked"));
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let pixels = pixels.expect("readback never completed - render world stalled");

        let has_red_pixel = pixels.chunks_exact(4).any(|px| px[0] > 200 && px[2] < 60);
        assert!(
            has_red_pixel,
            "rendered texture does not contain the drawn red background"
        );
    }
}

// ============================================================================
// Test: TuiRequest with TuiFontSource::Asset materializes once the font
// asset finishes loading (P1-2 acceptance). No GPU needed - materialization
// is pure CPU + Assets<Image>; the app here has no RenderPlugin at all.
// ============================================================================

#[cfg(test)]
mod tui_request_tests {
    use super::*;
    use crate::bevy_plugin::TerminalPlugin;

    #[test]
    fn asset_font_request_materializes_once_loaded() {
        let mut app = App::new();
        app.add_plugins((
            bevy::app::TaskPoolPlugin::default(),
            // Test fixture fonts live in examples/assets/ (shared with the
            // examples, see examples/retro_crt.rs's `build_app`), not the
            // AssetPlugin default `assets/` relative to the crate root.
            bevy::asset::AssetPlugin {
                file_path: "examples/assets".into(),
                ..default()
            },
            // Headless: registers the window message types
            // `window_resize_system` reads, without an OS window.
            bevy::window::WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            },
            bevy::image::ImagePlugin::default(),
            TerminalPlugin::display_only(),
        ));
        app.finish();
        app.cleanup();

        // Spawn the request BEFORE the font has loaded - the async load has
        // not even been polled yet at this point.
        let handle = app
            .world()
            .resource::<AssetServer>()
            .load("fonts/Mplus1Code-Regular.ttf");
        let entity = app
            .world_mut()
            .spawn(TuiRequest::headless(
                4,
                2,
                TuiFontSource::Asset {
                    handle,
                    size_px: 16,
                },
            ))
            .id();
        assert!(
            app.world().get::<Tui>(entity).is_none(),
            "Tui must not exist before the app ever updated"
        );

        let mut materialized = false;
        for _ in 0..500 {
            app.update();
            if app.world().get::<Tui>(entity).is_some() {
                materialized = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            materialized,
            "terminal never materialized - font asset did not load or \
             materialize_tui_requests did not run"
        );
        assert!(
            app.world().get::<TuiRequest>(entity).is_none(),
            "TuiRequest must be removed after materialization"
        );
    }
}

// ============================================================================
// Test: attach_terminal_system settles into zero archetype churn (P2-1).
//
// Component add/remove always re-stamps bevy's per-component change tick,
// even when the reinserted value is identical - so "no remove+insert
// commands after settling" is directly observable as "the component's
// `last_changed()` tick is unchanged across a second run", without needing
// to inspect archetype storage directly (a full remove-then-reinsert cycle
// always settles back into the *same* final archetype - same component
// set - so archetype identity alone can't distinguish churn from no-op;
// the change tick can, since it records *when* the value was last written,
// not just *what* the current component set is). Pure `World` +
// `RunSystemOnce`, no App/GPU needed.
// ============================================================================

#[cfg(all(test, feature = "3d"))]
mod attach_churn_tests {
    use super::*;
    use crate::fonts::{Font, Fonts};
    use bevy::ecs::change_detection::DetectChanges;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn settled_standard_attach_causes_no_archetype_churn() {
        let mut world = World::new();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<StandardMaterial>>();

        let font_data = include_bytes!("../examples/assets/fonts/Mplus1Code-Regular.ttf");
        let font = Font::new(font_data).expect("failed to load test font");
        let fonts = Arc::new(Fonts::new(font, 16));
        let texture = {
            let mut images = world.resource_mut::<Assets<Image>>();
            TerminalTexture::create(4, 2, fonts, false, false, [0, 0, 0, 255], &mut images)
                .expect("failed to create terminal texture")
        };
        let tui_entity = world.spawn(Tui::from_texture_state(texture)).id();

        // Simulate a mesh entity that already carries a stock material, as
        // e.g. a glTF loader would have inserted before `AttachTerminal`
        // gets a chance to claim it.
        let placeholder = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let surface = world
            .spawn((
                MeshMaterial3d(placeholder),
                AttachTerminal {
                    terminal: tui_entity,
                    material: AttachMaterial::standard(AlphaMode::Opaque),
                },
            ))
            .id();

        world
            .run_system_once(attach_terminal_system)
            .expect("first run (claim) failed");
        assert!(
            world.get::<TuiAttached>(surface).is_some(),
            "first run must record the claimed handle in TuiAttached"
        );
        let tick_after_first = world
            .entity(surface)
            .get_ref::<MeshMaterial3d<StandardMaterial>>()
            .expect("surface entity must carry a MeshMaterial3d<StandardMaterial>")
            .last_changed();

        // A real per-frame Update advances the world's change tick between
        // system runs - do the same here so a spurious re-insert on the
        // second run would actually get a *different* tick stamped, not
        // coincidentally the same one.
        world.increment_change_tick();

        world
            .run_system_once(attach_terminal_system)
            .expect("second run (settled) failed");
        let tick_after_second = world
            .entity(surface)
            .get_ref::<MeshMaterial3d<StandardMaterial>>()
            .expect("surface entity must still carry a MeshMaterial3d<StandardMaterial>")
            .last_changed();

        assert_eq!(
            tick_after_first, tick_after_second,
            "a settled AttachMaterial::standard() target must not be \
             re-inserted on a second run - the component's change tick \
             would have advanced if a spurious remove+insert happened"
        );
    }
}

// ============================================================================
// Test: runtime resize (P1-3). Pure CPU + Assets<Image>, no GPU/App needed -
// `TerminalTexture::resize`/`Tui::apply_pending_resize` touch neither.
// ============================================================================

#[cfg(test)]
mod resize_tests {
    use super::*;
    use crate::fonts::{Font, Fonts};

    fn test_fonts() -> Arc<Fonts> {
        let font_data = include_bytes!("../examples/assets/fonts/Mplus1Code-Regular.ttf");
        let font = Font::new(font_data).expect("failed to load test font");
        Arc::new(Fonts::new(font, 16))
    }

    #[test]
    fn request_resize_updates_grid_and_keeps_the_same_image_handle() {
        let mut images = Assets::<Image>::default();
        let texture = TerminalTexture::create(4, 2, test_fonts(), false, false, [0, 0, 0, 255], &mut images)
            .expect("failed to create terminal texture");
        let original_handle = texture.image_handle();
        let mut tui = Tui::from_texture_state(texture);

        tui.request_resize(8, 6);
        let applied = tui.apply_pending_resize(&mut images);
        assert_eq!(applied, Some((8, 6)), "resize must report the new grid size");
        assert_eq!(tui.grid_size(), (8, 6));
        assert_eq!(
            tui.image_handle(),
            &original_handle,
            "resize must recreate the Image at the SAME handle - no \
             ImageNode/material re-pointing needed downstream"
        );
        assert!(
            tui.size_px().x > 0 && tui.size_px().y > 0,
            "resized pixel dimensions must be non-zero"
        );
    }

    #[test]
    fn resize_to_the_current_size_is_a_no_op() {
        let mut images = Assets::<Image>::default();
        let texture = TerminalTexture::create(4, 2, test_fonts(), false, false, [0, 0, 0, 255], &mut images)
            .expect("failed to create terminal texture");
        let mut tui = Tui::from_texture_state(texture);

        tui.request_resize(4, 2); // same as creation size
        assert!(
            tui.apply_pending_resize(&mut images).is_none(),
            "requesting the current grid size must not queue a resize"
        );
    }
}

// ============================================================================
// Test: HitRegions (P2-4). Pure CPU, no bevy/GPU needed at all.
// ============================================================================

#[cfg(test)]
mod hit_regions_tests {
    use super::*;
    use ratatui::layout::Rect;

    #[derive(Debug, PartialEq)]
    enum WidgetId {
        A,
        B,
    }

    impl TryFrom<u64> for WidgetId {
        type Error = ();
        fn try_from(value: u64) -> Result<Self, Self::Error> {
            match value {
                0 => Ok(WidgetId::A),
                1 => Ok(WidgetId::B),
                _ => Err(()),
            }
        }
    }

    #[test]
    fn last_registered_wins_on_overlap() {
        let mut regions = HitRegions::default();
        regions.add(0u64, Rect::new(0, 0, 10, 10)); // A: (0,0)-(9,9)
        regions.add(1u64, Rect::new(5, 5, 10, 10)); // B: (5,5)-(14,14), overlaps A at (5,5)-(9,9)

        assert_eq!(
            regions.hit_at::<WidgetId>((6, 6)),
            Some(WidgetId::B),
            "the later (topmost) registration must win in the overlap"
        );
        assert_eq!(
            regions.hit_at::<WidgetId>((1, 1)),
            Some(WidgetId::A),
            "a point only inside the earlier region must still resolve to it"
        );
    }

    #[test]
    fn decode_failure_returns_none_without_falling_through() {
        let mut regions = HitRegions::default();
        regions.add(0u64, Rect::new(0, 0, 10, 10)); // A, decodes fine
        regions.add(99u64, Rect::new(0, 0, 10, 10)); // topmost, same area, undecodable id

        assert_eq!(
            regions.hit_at::<WidgetId>((5, 5)),
            None,
            "an undecodable topmost id must return None, not fall through to the \
             valid region underneath it"
        );
    }

    #[test]
    fn add_inner_excludes_the_block_border() {
        let mut regions = HitRegions::default();
        let block = ratatui::widgets::Block::bordered();
        regions.add_inner(0u64, &block, Rect::new(0, 0, 10, 10));

        assert_eq!(
            regions.hit_at::<WidgetId>((0, 0)),
            None,
            "the border cell itself must be excluded from the inner hit area"
        );
        assert_eq!(
            regions.hit_at::<WidgetId>((1, 1)),
            Some(WidgetId::A),
            "just inside the border must hit"
        );
    }

    #[test]
    fn clear_removes_all_regions() {
        let mut regions = HitRegions::default();
        regions.add(0u64, Rect::new(0, 0, 10, 10));
        regions.clear();

        assert_eq!(regions.hit_at::<WidgetId>((5, 5)), None);
    }
}
