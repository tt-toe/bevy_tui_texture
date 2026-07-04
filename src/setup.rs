//! Terminal texture creation and ECS spawn helpers.
//!
//! Two levels of abstraction for creating and managing terminals:
//!
//! 1. **`TerminalTexture`** - Core texture operations only
//!    - Just creates the terminal texture and ratatui Terminal
//!    - User must manually spawn entities and add input components
//!    - Maximum flexibility and control; wrap it in a [`Tui`] to draw with
//!      zero render-resource parameters per frame (`gpu_flush_system`,
//!      registered by `TerminalPlugin`, renders into the library-owned
//!      texture, then a render-world system copies it into the terminal's
//!      `Image`/`GpuImage` - no CPU readback, no material touch needed)
//!
//! 2. **`TerminalBundle::ui` / `TerminalBundle::world_quad`** - thin spawn
//!    helpers that create the texture, build the right components (`Tui`,
//!    `ImageNode`+`Node` or `Mesh3d`+`MeshMaterial3d<StandardMaterial>`,
//!    `TerminalInput`), and return a `Bundle` to `commands.spawn(...)` -
//!    with any extra marker components alongside in the same spawn call
//!
//! For attaching a `Tui` to an *existing* mesh (e.g. a glTF primitive) or a
//! custom material type, see [`AttachTerminal`]/[`AttachMaterial`].
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
//!     render_device: Res<RenderDevice>,
//!     render_queue: Res<RenderQueue>,
//!     mut images: ResMut<Assets<Image>>,
//! ) {
//!     let fonts = /* load fonts */;
//!
//!     let texture = TerminalTexture::create(
//!         80, 25, fonts, true,
//!         &render_device, &render_queue, &mut images,
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
//! ## Level 2: TerminalBundle::ui (2D UI Overlay)
//!
//! ```ignore
//! use bevy::prelude::*;
//! use bevy_tui_texture::prelude::*;
//!
//! fn setup(mut commands: Commands, render_device: Res<RenderDevice>,
//!     render_queue: Res<RenderQueue>, mut images: ResMut<Assets<Image>>,
//!     mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
//!     let mut ctx = TerminalSpawnCtx {
//!         render_device: &render_device, render_queue: &render_queue,
//!         images: &mut images, meshes: &mut meshes, materials: &mut materials,
//!     };
//!     let bundle = TerminalBundle::ui(80, 25, fonts, TerminalConfig::default(), &mut ctx).unwrap();
//!     commands.spawn((bundle, Node::default()));
//! }
//! ```

use std::sync::Arc;
#[cfg(feature = "3d")]
use std::sync::Mutex;

#[cfg(feature = "3d")]
use bevy::pbr::{Material, StandardMaterial};
use bevy::prelude::*;
use bevy::render::renderer::{RenderDevice, RenderQueue};

use crate::backend::bevy_backend::{BevyTerminalBackend, TerminalBuilder};
use crate::bevy_plugin::TerminalDimensions;
use crate::fonts::Fonts;
#[cfg(feature = "3d")]
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
    pub texture: wgpu::Texture,
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
    /// * `render_device` - Bevy's RenderDevice resource
    /// * `render_queue` - Bevy's RenderQueue resource
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
    /// # fn setup(render_device: Res<RenderDevice>, render_queue: Res<RenderQueue>, mut images: ResMut<Assets<Image>>) {
    /// let fonts = /* load fonts */;
    /// let texture = TerminalTexture::create(
    ///     80, 25, fonts, true,
    ///     &render_device, &render_queue, &mut images,
    /// ).unwrap();
    /// # }
    /// ```
    pub fn create(
        cols: u16,
        rows: u16,
        fonts: Arc<Fonts>,
        programmatic_glyphs: bool,
        render_device: &RenderDevice,
        render_queue: &RenderQueue,
        images: &mut Assets<Image>,
    ) -> Result<Self, crate::TerminalError> {
        let char_width_px = fonts.min_width_px();
        let char_height_px = fonts.height_px();
        let width = cols as u32 * char_width_px;
        let height = rows as u32 * char_height_px;

        // Create GPU texture
        let texture = render_device
            .wgpu_device()
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("Terminal Texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });

        // Render-world-only image: no CPU-side pixel data ever exists for
        // it. It starts wgpu-zero-initialized (transparent black, which
        // reads as plain black through any `AlphaMode::Opaque` material)
        // and is updated exclusively via the render-world GPU->GPU copy
        // system (`copy_tui_textures` in `bevy_plugin.rs`) - never through
        // bevy's normal CPU-upload asset pipeline. `COPY_SRC` is
        // deliberately absent: nothing ever copies data back out of this
        // destination texture.
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
        image.texture_descriptor.usage = bevy::render::render_resource::TextureUsages::COPY_DST
            | bevy::render::render_resource::TextureUsages::TEXTURE_BINDING;
        let image_handle = images.add(image);

        // Create backend
        let mut backend = TerminalBuilder::new(fonts)
            .with_dimensions(cols, rows)
            .build(render_device.wgpu_device(), render_queue.0.as_ref());

        // Optionally pre-populate programmatic glyphs
        if programmatic_glyphs {
            backend.populate_programmatic_glyphs(render_queue.0.as_ref());
        }

        let terminal = ratatui::Terminal::new(backend)
            ?;

        Ok(Self {
            terminal,
            texture,
            image_handle,
            width,
            height,
            cols,
            rows,
            char_width_px,
            char_height_px,
        })
    }

    /// Render the terminal's current ratatui buffer to the library-owned
    /// GPU texture. Synchronous (a GPU render pass, not a readback) -
    /// called by [`Tui::flush`] whenever the terminal is dirty.
    fn render_to_texture(&mut self, render_device: &RenderDevice, render_queue: &RenderQueue) {
        let texture_view = self
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.terminal.backend_mut().render_to_texture(
            render_device.wgpu_device(),
            render_queue.0.as_ref(),
            &texture_view,
        );
    }

    /// Read the library-owned texture's current pixels back to the CPU,
    /// **blocking** until the copy completes. Opt-in only (screenshots,
    /// tests) - never call this every frame, it stalls the CPU on the GPU.
    /// Returns tightly-packed RGBA8 bytes (`width * height * 4`), with any
    /// wgpu row padding already stripped.
    pub(crate) fn read_back_pixels_blocking(
        &self,
        render_device: &RenderDevice,
        render_queue: &RenderQueue,
    ) -> Vec<u8> {
        let unpadded_bytes_per_row = self.width * 4;
        let bytes_per_row = {
            let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
            let padding = (align - (unpadded_bytes_per_row % align)) % align;
            unpadded_bytes_per_row + padding
        };
        let buffer_size = (bytes_per_row * self.height) as wgpu::BufferAddress;

        let staging_buffer = render_device
            .wgpu_device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("Terminal Readback Buffer"),
                size: buffer_size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });

        let mut encoder =
            render_device
                .wgpu_device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Terminal Readback Encoder"),
                });
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &staging_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        render_queue.0.submit(Some(encoder.finish()));

        let buffer_slice = staging_buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).ok();
        });
        render_device
            .wgpu_device()
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .ok();
        receiver
            .recv()
            .expect("map_async callback never ran")
            .expect("failed to map readback buffer");

        let mapped = buffer_slice.get_mapped_range();
        let mut out = vec![0u8; (unpadded_bytes_per_row * self.height) as usize];
        if bytes_per_row == unpadded_bytes_per_row {
            out.copy_from_slice(&mapped);
        } else {
            for y in 0..self.height {
                let src_offset = (y * bytes_per_row) as usize;
                let dst_offset = (y * unpadded_bytes_per_row) as usize;
                out[dst_offset..dst_offset + unpadded_bytes_per_row as usize].copy_from_slice(
                    &mapped[src_offset..src_offset + unpadded_bytes_per_row as usize],
                );
            }
        }
        drop(mapped);
        staging_buffer.unmap();
        out
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
/// readback, no material touch (see `IMPROVEMENT.md` for why this replaced
/// the old async-copy architecture).
#[derive(Component)]
pub struct Tui {
    texture_state: TerminalTexture,
    dirty: bool,
    /// Set by `flush` whenever it renders a fresh frame into the
    /// library-owned texture; drained by the render-world extract system
    /// (`extract_tui_copies` in `bevy_plugin.rs`), which is the only other
    /// reader/writer of this flag.
    copy_pending: bool,
    hit_regions: HitRegions,
    /// Set once a `draw()`/`draw_with_hits()` call has logged its ratatui
    /// error, so a terminal that keeps failing every frame doesn't spam the
    /// log - one `warn!` per terminal is enough to diagnose it.
    draw_error_logged: bool,
}

impl Tui {
    /// Wrap an already-created [`TerminalTexture`]. This is the
    /// manual-entity-management constructor; for ergonomic spawning see the
    /// `TerminalBundle::ui`/`world_quad` helpers instead.
    pub fn from_texture_state(texture_state: TerminalTexture) -> Self {
        Self {
            texture_state,
            dirty: false,
            copy_pending: false,
            hit_regions: HitRegions::default(),
            draw_error_logged: false,
        }
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

    /// Escape hatch for binding the library-owned texture into your own
    /// render passes. Note this is the *source* texture this `Tui` renders
    /// into every dirty frame, not the destination `Image`/`GpuImage` the
    /// render world copies into - the two are kept in sync automatically,
    /// but they are different GPU textures.
    pub fn wgpu_texture(&self) -> &wgpu::Texture {
        &self.texture_state.texture
    }

    /// Read this terminal's current pixels back to the CPU, **blocking**
    /// until the GPU copy completes. An explicit opt-in for screenshots and
    /// tests only - the normal per-frame path never touches the CPU at all;
    /// do not call this every frame. Returns tightly-packed RGBA8 bytes.
    pub fn read_back_blocking(&self, render_device: &RenderDevice, render_queue: &RenderQueue) -> Vec<u8> {
        self.texture_state
            .read_back_pixels_blocking(render_device, render_queue)
    }

    /// Called by [`gpu_flush_system`](crate::bevy_plugin::gpu_flush_system).
    /// If dirty, renders into the library-owned texture and marks a copy as
    /// pending for the render-world extract system to pick up; the actual
    /// `Image`/`GpuImage` update happens there, not here.
    pub(crate) fn flush(&mut self, render_device: &RenderDevice, render_queue: &RenderQueue) {
        if self.dirty {
            self.texture_state
                .render_to_texture(render_device, render_queue);
            self.copy_pending = true;
            self.dirty = false;
        }
    }

    /// Drain the pending-copy flag, if set, returning the source texture
    /// (cloned - `wgpu::Texture` is a cheap `Arc`-backed handle), the
    /// destination image's asset id, and the copy size. Called once per
    /// frame by the render-world extract system
    /// (`extract_tui_copies` in `bevy_plugin.rs`) via `ResMut<MainWorld>`.
    pub(crate) fn take_copy_pending(&mut self) -> Option<(wgpu::Texture, AssetId<Image>, wgpu::Extent3d)> {
        if !self.copy_pending {
            return None;
        }
        self.copy_pending = false;
        Some((
            self.texture_state.texture.clone(),
            self.texture_state.image_handle.id(),
            wgpu::Extent3d {
                width: self.texture_state.width,
                height: self.texture_state.height,
                depth_or_array_layers: 1,
            },
        ))
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
// Config struct + thin spawn helpers
// ============================================================================

/// Configuration for [`TerminalBundle::ui`]/[`TerminalBundle::world_quad`].
/// A single struct instead of a long run of positional bools, so call sites
/// don't need to annotate every argument with a comment to stay readable.
pub struct TerminalConfig {
    /// Pre-populate box-drawing, braille, and powerline glyphs.
    pub programmatic_glyphs: bool,
    /// Whether this terminal can receive keyboard input (focus required).
    pub keyboard: bool,
    /// Whether this terminal can receive mouse input.
    pub mouse: bool,
    /// Drawn once at creation time (before the entity's own draw system
    /// runs), so the very first presented frame already has real content
    /// instead of the create-time fill color.
    pub initial_draw: Option<Box<dyn FnOnce(&mut ratatui::Frame) + Send>>,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            programmatic_glyphs: true,
            keyboard: true,
            mouse: true,
            initial_draw: None,
        }
    }
}

/// Borrowed handles to the resources every spawn helper needs, bundled into
/// one value so call sites don't thread five separate parameters through.
///
/// This holds plain references rather than being a `#[derive(SystemParam)]`
/// struct: a `SystemParam` can only be fetched directly from a system's own
/// signature, which would force every caller's system to take a
/// `TerminalSpawnCtx` as one of its *own* parameters and forbid also holding
/// `Res<RenderDevice>` etc. directly (bevy rejects the resulting duplicate/
/// conflicting access). Plain references compose freely: build one from
/// whatever `Res`/`ResMut` (or another `TerminalSpawnCtx`) your system
/// already has, no matter how many other resources it also needs.
///
/// Gated behind `all(2d, 3d)` as a whole, not split per-field: this is the
/// immediate-mode spawn path (see [`TerminalBundle`]); the declarative
/// `TuiRequest` alternative gates its variants individually.
#[cfg(all(feature = "2d", feature = "3d"))]
pub struct TerminalSpawnCtx<'w> {
    pub render_device: &'w RenderDevice,
    pub render_queue: &'w RenderQueue,
    pub images: &'w mut Assets<Image>,
    pub meshes: &'w mut Assets<Mesh>,
    pub materials: &'w mut Assets<StandardMaterial>,
}

/// Marker for [`TerminalBundle::ui`]-spawned entities. Requires `Node`: if
/// the caller's own spawn tuple includes a `Node`, that one wins (bevy only
/// auto-inserts a required component when the entity doesn't already have
/// one); otherwise a default `Node` is inserted so the entity is still a
/// valid UI node.
#[cfg(feature = "2d")]
#[derive(Component, Default)]
#[require(Node)]
pub struct TuiUi;

/// Thin spawn helpers: create the texture and assemble components, nothing
/// more (no positioning, no god-function). Not a real bundle-deriving type;
/// just a namespace for the two constructors below, which each return
/// `impl Bundle` for the caller to spawn directly.
#[cfg(all(feature = "2d", feature = "3d"))]
pub struct TerminalBundle;

#[cfg(all(feature = "2d", feature = "3d"))]
impl TerminalBundle {
    /// 2D (bevy_ui) terminal. The returned bundle carries no `Node` of its
    /// own (see [`TuiUi`]) - place it with ordinary bevy_ui components in
    /// the same `spawn` tuple, e.g.:
    ///
    /// ```ignore
    /// commands.spawn((
    ///     TerminalBundle::ui(28, 10, fonts, TerminalConfig::default(), &mut ctx)?,
    ///     Node {
    ///         position_type: PositionType::Absolute,
    ///         right: Val::Px(20.0),
    ///         top: Val::Px(20.0),
    ///         ..default()
    ///     },
    /// ));
    /// ```
    pub fn ui(
        cols: u16,
        rows: u16,
        fonts: Arc<Fonts>,
        config: TerminalConfig,
        ctx: &mut TerminalSpawnCtx,
    ) -> Result<impl Bundle, crate::TerminalError> {
        let texture_state = TerminalTexture::create(
            cols,
            rows,
            fonts,
            config.programmatic_glyphs,
            ctx.render_device,
            ctx.render_queue,
            ctx.images,
        )?;

        let image_node = ImageNode {
            image: texture_state.image_handle(),
            ..default()
        };
        let dimensions = texture_state.dimensions();
        let mut tui = Tui::from_texture_state(texture_state);
        if let Some(initial_draw) = config.initial_draw {
            tui.draw(initial_draw);
        }

        Ok((
            tui,
            TuiUi,
            image_node,
            dimensions,
            TerminalInput {
                keyboard: config.keyboard,
                mouse: config.mouse,
            },
        ))
    }

    /// 3D quad sized in **world units** (`WorldTerminal3D`'s semantics):
    /// `height` in world units, width follows the texture's pixel aspect
    /// ratio. The quad's visible face normal is local `+Z` (matching the
    /// legacy `WorldTerminal3D`, whose already-verified UV/raycast mapping
    /// this reuses unchanged) - orient it with an ordinary `Transform` in
    /// the same `spawn` tuple, e.g. to face a camera:
    /// `Transform::from_translation(pos).with_rotation(Quat::from_rotation_arc(Vec3::Z, camera_pos - pos))`.
    /// (`Transform::looking_at` aligns local `-Z` with the target - the
    /// *opposite* convention - and would show the quad's back.)
    pub fn world_quad(
        cols: u16,
        rows: u16,
        fonts: Arc<Fonts>,
        height: f32,
        config: TerminalConfig,
        ctx: &mut TerminalSpawnCtx,
    ) -> Result<impl Bundle, crate::TerminalError> {
        let texture_state = TerminalTexture::create(
            cols,
            rows,
            fonts,
            config.programmatic_glyphs,
            ctx.render_device,
            ctx.render_queue,
            ctx.images,
        )?;

        let aspect = texture_state.width as f32 / texture_state.height as f32;
        let half_height = height / 2.0;
        let mesh = ctx.meshes.add(Plane3d::new(
            Vec3::Z,
            Vec2::new(half_height * aspect, half_height),
        ));
        let material = ctx.materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(texture_state.image_handle()),
            // Terminal content should not depend on scene lighting.
            unlit: true,
            alpha_mode: AlphaMode::Opaque,
            double_sided: true,
            cull_mode: None,
            ..default()
        });
        let dimensions = texture_state.dimensions();
        let mut tui = Tui::from_texture_state(texture_state);
        if let Some(initial_draw) = config.initial_draw {
            tui.draw(initial_draw);
        }

        Ok((
            tui,
            Mesh3d(mesh),
            MeshMaterial3d(material),
            dimensions,
            TerminalInput {
                keyboard: config.keyboard,
                mouse: config.mouse,
            },
        ))
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
/// name `M`, and would duplicate the per-type registration
/// `TerminalMaterialPlugin::<M>` already provides.
#[cfg(feature = "3d")]
pub struct AttachMaterial(UntypedMaterialInsert);

#[cfg(feature = "3d")]
impl AttachMaterial {
    /// Plain `StandardMaterial`, unlit, textured with the terminal.
    pub fn standard() -> Self {
        Self::custom(|image| StandardMaterial {
            base_color_texture: Some(image),
            unlit: true,
            alpha_mode: AlphaMode::Opaque,
            ..default()
        })
    }

    /// Any material type. `factory` builds a concrete material `M`
    /// (registered via `TerminalMaterialPlugin::<M>` if you want automatic
    /// per-frame touching) from the terminal's image handle.
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
                world.entity_mut(entity).insert(MeshMaterial3d(handle));
            },
        )))
    }
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
/// 3. RE-CLAIMS every frame while the entity still carries
///    `MeshMaterial3d<StandardMaterial>` (e.g. a glTF loader asynchronously
///    re-inserting its own stock material over ours - see CLAUDE.md
///    "Common Gotchas" #8), until the swap sticks and it drops out of the
///    query (for [`AttachMaterial::custom`] targets of a type other than
///    `StandardMaterial`) or settles into harmless no-op re-assertion of
///    the same cached handle (for [`AttachMaterial::standard`]).
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
    to_attach: Query<(Entity, &AttachTerminal), With<MeshMaterial3d<StandardMaterial>>>,
    terminals: Query<&Tui>,
) {
    for (surface_entity, attach) in &to_attach {
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
// Tests: a terminal drawn once and never again must still show its final
// content (i.e. static content must not be silently dropped by the async
// flush path).
//
// These need a real wgpu adapter/device - built directly from a bare wgpu
// Instance rather than a full bevy App/RenderPlugin, since that's the
// minimum needed to construct RenderDevice/RenderQueue. Skips (rather than
// fails) when no adapter is available, matching the deferred plan to not
// make CI depend on GPU availability.
// ============================================================================

#[cfg(test)]
mod tui_flush_tests {
    use super::*;
    use crate::fonts::{Font, Fonts};
    use bevy::render::renderer::WgpuWrapper;
    use ratatui::style::{Color as RatatuiColor, Style};
    use ratatui::widgets::Block;

    /// Best-effort headless GPU setup. Returns `None` (causing the test to
    /// skip, not fail) when no adapter is available in this environment.
    fn try_gpu() -> Option<(RenderDevice, RenderQueue)> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(
            instance.request_adapter(&wgpu::RequestAdapterOptions::default()),
        )
        .ok()?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
        Some((
            RenderDevice::from(device),
            RenderQueue(Arc::new(WgpuWrapper::new(queue))),
        ))
    }

    /// A repeatedly-failing `draw()` logs its error only once per `Tui`,
    /// not once per call - `log_draw_error` is the de-duplication gate this
    /// asserts on directly (a real ratatui backend failure is impractical
    /// to trigger from the outside, so this drives the same private method
    /// `draw`/`draw_with_hits` call on error).
    #[test]
    fn draw_error_logged_only_once() {
        let Some((render_device, render_queue)) = try_gpu() else {
            eprintln!("skipping: no GPU adapter available in this environment");
            return;
        };

        let font_data = include_bytes!("../assets/fonts/Mplus1Code-Regular.ttf");
        let font = Font::new(font_data).expect("failed to load test font");
        let fonts = Arc::new(Fonts::new(font, 16));

        let mut images = Assets::<Image>::default();
        let texture_state =
            TerminalTexture::create(4, 2, fonts, false, &render_device, &render_queue, &mut images)
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

    /// Regression test: draw once, flush once, then read back the
    /// library-owned texture directly (bypassing the render world - this
    /// bare-device test has none). The rendered content must be visible
    /// without ever touching the `Image` asset, confirming the Phase A
    /// render_to_texture + `read_back_blocking` path works end to end.
    #[test]
    fn flush_renders_drawn_content_synchronously() {
        let Some((render_device, render_queue)) = try_gpu() else {
            eprintln!("skipping: no GPU adapter available in this environment");
            return;
        };

        let font_data = include_bytes!("../assets/fonts/Mplus1Code-Regular.ttf");
        let font = Font::new(font_data).expect("failed to load test font");
        let fonts = Arc::new(Fonts::new(font, 16));

        let mut images = Assets::<Image>::default();
        let texture_state = TerminalTexture::create(
            4,
            2,
            fonts,
            false,
            &render_device,
            &render_queue,
            &mut images,
        )
        .expect("failed to create terminal texture");
        let mut tui = Tui::from_texture_state(texture_state);

        // Draw distinctive content and flush exactly once.
        tui.draw(|frame| {
            frame.render_widget(
                Block::default().style(Style::default().bg(RatatuiColor::Red)),
                frame.area(),
            );
        });
        tui.flush(&render_device, &render_queue);

        let pixels = tui.read_back_blocking(&render_device, &render_queue);
        let has_red_pixel = pixels.chunks_exact(4).any(|px| px[0] > 200 && px[2] < 60);
        assert!(
            has_red_pixel,
            "rendered texture does not contain the drawn red background"
        );
    }

    /// No-change skip (design point 7): redrawing byte-identical content
    /// must not re-mark the terminal dirty, so `flush` performs no GPU
    /// work on the second call. Needs a real texture only to construct
    /// `Tui` - `draw()`'s dirty tracking itself is pure CPU/ratatui logic.
    #[test]
    fn identical_redraw_does_not_mark_dirty() {
        let Some((render_device, render_queue)) = try_gpu() else {
            eprintln!("skipping: no GPU adapter available in this environment");
            return;
        };

        let font_data = include_bytes!("../assets/fonts/Mplus1Code-Regular.ttf");
        let font = Font::new(font_data).expect("failed to load test font");
        let fonts = Arc::new(Fonts::new(font, 16));

        let mut images = Assets::<Image>::default();
        let texture_state = TerminalTexture::create(
            4,
            2,
            fonts,
            false,
            &render_device,
            &render_queue,
            &mut images,
        )
        .expect("failed to create terminal texture");
        let mut tui = Tui::from_texture_state(texture_state);

        let paint_red =
            |frame: &mut ratatui::Frame| {
                frame.render_widget(
                    Block::default().style(Style::default().bg(RatatuiColor::Red)),
                    frame.area(),
                );
            };

        // First draw: empty -> red is definitely a change.
        tui.draw(paint_red);
        assert!(tui.dirty, "first draw onto an empty buffer must be dirty");
        tui.flush(&render_device, &render_queue);
        assert!(!tui.dirty, "flush must clear dirty");

        // Second draw: byte-identical content -> zero-cell diff -> must
        // NOT re-mark dirty (this is the whole point of the optimization).
        tui.draw(paint_red);
        assert!(
            !tui.dirty,
            "identical redraw must not mark dirty (no-change skip)"
        );
    }
}
