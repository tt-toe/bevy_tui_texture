//! Terminal texture creation and ECS spawning.

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
        image.texture_descriptor.usage =
            bevy::render::render_resource::TextureUsages::RENDER_ATTACHMENT
                | bevy::render::render_resource::TextureUsages::TEXTURE_BINDING
                | bevy::render::render_resource::TextureUsages::COPY_SRC;
        let image_handle = images.add(image);

        let mut backend = TerminalBuilder::new(fonts)
            .with_dimensions(cols, rows)
            .with_transparent_reset_bg(transparent_reset_bg)
            .with_initial_fill(initial_fill)
            .build();

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
    pub fn dimensions(&self) -> TerminalDimensions {
        TerminalDimensions {
            cols: self.cols,
            rows: self.rows,
            char_width_px: self.char_width_px,
            char_height_px: self.char_height_px,
        }
    }

    /// Get the image handle for entity setup.
    pub fn image_handle(&self) -> Handle<Image> {
        self.image_handle.clone()
    }

    /// Resize to a new grid size in place.
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
        image.texture_descriptor.usage =
            bevy::render::render_resource::TextureUsages::RENDER_ATTACHMENT
                | bevy::render::render_resource::TextureUsages::TEXTURE_BINDING
                | bevy::render::render_resource::TextureUsages::COPY_SRC;
        images
            .insert(&self.image_handle, image)
            .expect("resize: destination image handle must still be valid");
    }
}

/// Registry mapping click regions to caller-defined `u64` ids, rebuilt on
/// every [`Tui::draw_with_hits`] call.
#[derive(Component, Default)]
pub struct HitRegions {
    regions: Vec<(u64, ratatui::layout::Rect)>,
}

impl HitRegions {
    pub fn clear(&mut self) {
        self.regions.clear();
    }

    /// Register a hit region.
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

    /// Decodes the topmost (last-registered) region containing `pos`.
    pub fn hit_at<T: TryFrom<u64>>(&self, pos: (u16, u16)) -> Option<T> {
        let point = ratatui::layout::Position { x: pos.0, y: pos.1 };
        self.regions
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(point))
            .and_then(|(id, _)| T::try_from(*id).ok())
    }
}

/// One terminal = one Entity.
#[derive(Component)]
pub struct Tui {
    texture_state: TerminalTexture,
    /// Starts `true` (not `false`): the uninitialized-texture guard.
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
    /// Wrap an already-created [`TerminalTexture`].
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

    /// Request a new grid size.
    pub fn request_resize(&mut self, cols: u16, rows: u16) {
        let (current_cols, current_rows) = self.grid_size();
        if (cols, rows) != (current_cols, current_rows) {
            self.pending_resize = Some((cols, rows));
        }
    }

    /// Whether a [`Self::request_resize`] is waiting for `gpu_flush_system`.
    pub(crate) fn has_pending_resize(&self) -> bool {
        self.pending_resize.is_some()
    }

    /// Whether a draw since the last flush changed any cell.
    pub(crate) fn needs_flush(&self) -> bool {
        self.dirty
    }

    /// Whether a flushed payload is waiting for `extract_tui_draws`.
    pub(crate) fn has_pending_draw(&self) -> bool {
        self.pending_draw.is_some()
    }

    /// Apply a pending resize, if any: recreate the destination `Image` at
    /// the new pixel size, update the backend's grid dimensions, and resize
    /// ratatui's own buffers immediately (rather than waiting for the next
    /// `Terminal::draw`'s `autoresize` to notice the mismatch) so the very
    /// next `Tui::draw` call already sees the new grid.
    pub(crate) fn apply_pending_resize(
        &mut self,
        images: &mut Assets<Image>,
    ) -> Option<(u16, u16)> {
        let (cols, rows) = self.pending_resize.take()?;
        self.texture_state.resize(cols, rows, images);
        let backend = self.texture_state.terminal.backend_mut();
        backend.resize(cols, rows);

        let mut payload = backend.take_draw_payload();
        payload.discard_stale_geometry();
        self.pending_draw = Some(payload);

        backend.force_full_redraw();

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

    /// Draw with ratatui.
    pub fn draw(&mut self, f: impl FnOnce(&mut ratatui::Frame)) {
        match self.texture_state.terminal.draw(f) {
            Ok(_) => self.mark_dirty_if_changed(),
            Err(err) => self.log_draw_error(err),
        }
    }

    /// `draw()` variant handing the caller a `&mut HitRegions` alongside the
    /// `Frame`, so click regions are registered right next to the
    /// `render_widget` call that draws them.
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
        if self
            .texture_state
            .terminal
            .backend()
            .cells_changed_last_draw()
        {
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
    /// until the render world performs the copy.
    pub fn read_back_blocking(&self, channel: &crate::bevy_plugin::TuiReadbackChannel) -> Vec<u8> {
        channel.request_blocking(self.texture_state.image_handle.id())
    }

    /// Called by [`gpu_flush_system`](crate::bevy_plugin::gpu_flush_system).
    pub(crate) fn flush(&mut self) {
        if self.dirty {
            let backend = self.texture_state.terminal.backend_mut();
            if self.pending_draw.is_some() {
                // About to overwrite a not-yet-extracted payload (the
                // render world skipped a frame - it extracts every frame
                // once running, but sits out the first few while the
                // renderer initializes asynchronously). A partial payload
                // only covers the rows dirty as of ITS take - if the new
                // payload were also partial, the overwritten payload's
                // rows would simply be dropped, never rendered. Force a
                // full payload instead, which safely supersedes anything
                // (glyph rasterizations live in the shared per-font state,
                // `Fonts`, rather than in this payload, so there is
                // nothing font-side to lose here either way).
                backend.force_full_redraw();
            }
            let draw = backend.take_draw_payload();
            self.pending_draw = Some(draw);
            self.dirty = false;
        }
    }

    /// Drain the pending draw payload, if set, returning it alongside the
    /// destination image's asset id.
    pub(crate) fn take_pending_draw(
        &mut self,
    ) -> Option<(AssetId<Image>, crate::backend::TerminalDrawPayload)> {
        self.pending_draw
            .take()
            .map(|draw| (self.texture_state.image_handle.id(), draw))
    }

    /// Returns this terminal's font identity key, alongside any glyph
    /// rasterizations still pending upload to that font's shared atlas.
    pub(crate) fn take_shared_font_uploads(
        &self,
    ) -> (usize, Vec<(crate::utils::text_atlas::CacheRect, Vec<u8>)>) {
        let backend = self.texture_state.terminal.backend();
        (backend.font_key(), backend.take_shared_glyph_uploads())
    }

    /// [`Self::take_shared_font_uploads`] for the color glyph atlas: its
    /// texture size, and the queued rasterizations.
    #[cfg(feature = "emoji")]
    pub(crate) fn take_shared_color_uploads(
        &self,
    ) -> (
        (u32, u32),
        Vec<(crate::utils::text_atlas::CacheRect, Vec<u8>)>,
    ) {
        self.texture_state
            .terminal
            .backend()
            .take_shared_color_uploads()
    }
}

/// Marks an entity as the visible *surface* for a terminal, when the surface
/// and the [`Tui`] component live on different entities (the "attach to an
/// existing mesh" case, see [`AttachTerminal`]).
#[derive(Component, Debug, Clone, Copy)]
pub struct TuiSurface {
    pub tui: Entity,
}

/// Configuration for [`TuiRequest`].
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
    pub initial_draw: Option<Box<dyn FnOnce(&mut ratatui::Frame) + Send + Sync>>,
    /// Color shown before any content has been drawn (or, transiently,
    /// mid-resize - see `Tui::request_resize`).
    pub initial_fill: [u8; 4],
    /// If `true`, cells with no explicit background (`ratatui::style::
    /// Color::Reset`, ratatui's own default) render with alpha 0 instead
    /// of an opaque fill color - letting a `TuiKind::WorldQuad` terminal
    /// with `alpha_mode: AlphaMode::Blend` show the scene through its
    /// background, or a `TuiKind::Ui` terminal show through to whatever is
    /// behind its `Node`.
    pub transparent_reset_bg: bool,
    /// Alpha/transparency mode for the material [`TuiKind::WorldQuad`]
    /// builds.
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
    /// on Wasm).
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
    /// follows the texture's pixel aspect ratio.
    #[cfg(feature = "3d")]
    WorldQuad { height: f32 },
    /// A `Tui` with no surface components of its own - for terminals whose
    /// display surface is an existing mesh claimed via [`AttachTerminal`],
    /// or a fully custom entity setup around
    /// [`Tui::image_handle`](Tui::image_handle).
    Headless,
}

/// Records a [`TuiKind::WorldQuad`] terminal's configured world-unit height,
/// inserted at materialization.
#[cfg(feature = "3d")]
#[derive(Component, Clone, Copy)]
pub(crate) struct WorldQuadHeight(pub(crate) f32);

/// Declarative terminal request: spawn this component (plus any `Node` /
/// `Transform` / marker components you want on the terminal entity), and
/// the plugin's `materialize_tui_requests` system does the rest - **your
/// systems never touch `Assets<Image>` or any other render resource**.
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

/// Marker for [`TuiKind::Ui`] terminals.
#[cfg(feature = "2d")]
#[derive(Component, Default)]
#[require(Node)]
pub struct TuiUi;

/// Plugin system backing [`TuiRequest`].
pub(crate) fn materialize_tui_requests(
    mut commands: Commands,
    mut requests: Query<(Entity, &mut TuiRequest), Without<Tui>>,
    asset_server: Res<AssetServer>,
    font_assets: Res<Assets<crate::fonts::TerminalFontAsset>>,
    mut images: ResMut<Assets<Image>>,
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
                    continue;
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
                let (Some(meshes), Some(materials)) = (meshes.as_mut(), materials.as_mut()) else {
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

/// Type-erased "insert this material" action.
#[cfg(feature = "3d")]
#[derive(Clone)]
struct UntypedMaterialInsert(
    std::sync::Arc<dyn Fn(Handle<Image>, Entity, &mut World) + Send + Sync>,
);

/// How to material a [`AttachTerminal`]-marked mesh.
#[cfg(feature = "3d")]
pub struct AttachMaterial(UntypedMaterialInsert);

#[cfg(feature = "3d")]
impl AttachMaterial {
    /// Plain `StandardMaterial`, unlit, textured with the terminal.
    pub fn standard(alpha_mode: AlphaMode) -> Self {
        Self::custom(move |image| StandardMaterial {
            base_color_texture: Some(image),
            unlit: true,
            alpha_mode,
            ..default()
        })
    }

    /// Any material type.
    pub fn custom<M: Material>(
        factory: impl Fn(Handle<Image>) -> M + Send + Sync + 'static,
    ) -> Self {
        let cached: Mutex<Option<Handle<M>>> = Mutex::new(None);
        AttachMaterial(UntypedMaterialInsert(std::sync::Arc::new(
            move |image, entity, world| {
                let mut cached = cached.lock().unwrap();
                let handle = match cached.as_ref() {
                    Some(handle) => handle.clone(),
                    None => {
                        let handle = world.resource_mut::<Assets<M>>().add(factory(image));
                        *cached = Some(handle.clone());
                        handle
                    }
                };
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
/// on every settled frame.
#[cfg(feature = "3d")]
#[derive(Component)]
pub(crate) struct TuiAttached {
    material: UntypedHandle,
}

/// Insert on a mesh entity (e.g. a glTF primitive) to attach a `Tui` to it.
#[cfg(feature = "3d")]
#[derive(Component)]
pub struct AttachTerminal {
    /// Entity carrying the `Tui` component to display on this mesh.
    pub terminal: Entity,
    pub material: AttachMaterial,
}

/// Plugin system backing [`AttachTerminal`].
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
            continue;
        }

        let Ok(tui) = terminals.get(attach.terminal) else {
            continue;
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

// Mesa 26's lavapipe reports features it cannot create a device with, and
// bevy's default priority requests every reported feature.
#[cfg(test)]
fn test_render_plugin() -> bevy::render::RenderPlugin {
    use bevy::render::settings::{WgpuSettings, WgpuSettingsPriority};
    bevy::render::RenderPlugin {
        render_creation: WgpuSettings {
            priority: WgpuSettingsPriority::WebGL2,
            ..Default::default()
        }
        .into(),
        ..Default::default()
    }
}

// The Vulkan loader crashes when tests create and destroy devices concurrently.
#[cfg(test)]
fn gpu_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

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

    #[test]
    fn draw_error_logged_only_once() {
        let mut images = Assets::<Image>::default();
        let texture_state = TerminalTexture::create(
            4,
            2,
            test_fonts(),
            false,
            false,
            [0, 0, 0, 255],
            &mut images,
        )
        .expect("failed to create terminal texture");
        let mut tui = Tui::from_texture_state(texture_state);

        assert!(!tui.draw_error_logged);
        let fake_err = || std::io::Error::other("simulated backend failure");
        tui.log_draw_error(fake_err());
        assert!(tui.draw_error_logged, "first error must flip the flag");
        tui.log_draw_error(fake_err());
        assert!(tui.draw_error_logged);
    }

    #[test]
    fn identical_redraw_does_not_mark_dirty() {
        let mut images = Assets::<Image>::default();
        let texture_state = TerminalTexture::create(
            4,
            2,
            test_fonts(),
            false,
            false,
            [0, 0, 0, 255],
            &mut images,
        )
        .expect("failed to create terminal texture");
        let mut tui = Tui::from_texture_state(texture_state);

        let paint_red = |frame: &mut ratatui::Frame| {
            frame.render_widget(
                Block::default().style(Style::default().bg(RatatuiColor::Red)),
                frame.area(),
            );
        };

        tui.draw(paint_red);
        assert!(tui.dirty, "first draw onto an empty buffer must be dirty");
        tui.flush();
        assert!(!tui.dirty, "flush must clear dirty");

        tui.draw(paint_red);
        assert!(
            !tui.dirty,
            "identical redraw must not mark dirty (no-change skip)"
        );
    }

    #[test]
    fn flush_renders_drawn_content_synchronously() {
        let _gpu = gpu_test_lock();
        use crate::bevy_plugin::TerminalPlugin;
        use crate::bevy_plugin::TuiReadbackChannel;
        use bevy::render::renderer::RenderDevice;

        let mut app = App::new();
        app.add_plugins((
            bevy::app::TaskPoolPlugin::default(),
            bevy::asset::AssetPlugin::default(),
            bevy::window::WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            },
            bevy::mesh::MeshPlugin,
            bevy::diagnostic::FrameCountPlugin,
            bevy::time::TimePlugin,
            super::test_render_plugin(),
            bevy::image::ImagePlugin::default(),
            TerminalPlugin::display_only(),
        ));
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
            let texture_state = TerminalTexture::create(
                4,
                2,
                test_fonts(),
                false,
                false,
                [0, 0, 0, 255],
                &mut images,
            )
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

        let image_id = app.world().get::<Tui>(entity).unwrap().image_handle().id();
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

        let has_red_pixel = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|px| px[0] > 200 && px[2] < 60);
        assert!(
            has_red_pixel,
            "rendered texture does not contain the drawn red background"
        );
    }

    fn gpu_app() -> Option<App> {
        use crate::bevy_plugin::TerminalPlugin;
        use bevy::render::renderer::RenderDevice;

        let mut app = App::new();
        app.add_plugins((
            bevy::app::TaskPoolPlugin::default(),
            bevy::asset::AssetPlugin::default(),
            bevy::window::WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            },
            bevy::mesh::MeshPlugin,
            bevy::diagnostic::FrameCountPlugin,
            bevy::time::TimePlugin,
            super::test_render_plugin(),
            bevy::image::ImagePlugin::default(),
            TerminalPlugin::display_only(),
        ));
        app.init_resource::<bevy::camera::ClearColor>();
        app.finish();
        app.cleanup();

        for _ in 0..200 {
            app.update();
            if app.world().get_resource::<RenderDevice>().is_some() {
                return Some(app);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        eprintln!("skipping: no GPU adapter available in this environment");
        None
    }

    fn read_back(app: &mut App, entity: Entity) -> Vec<u8> {
        use crate::bevy_plugin::TuiReadbackChannel;

        let image_id = app.world().get::<Tui>(entity).unwrap().image_handle().id();
        let channel = app.world().resource::<TuiReadbackChannel>().clone();
        let readback = std::thread::spawn(move || channel.request_blocking(image_id));
        for _ in 0..200 {
            app.update();
            if readback.is_finished() {
                return readback.join().expect("readback thread panicked");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("readback never completed - render world stalled");
    }

    #[test]
    fn cells_resolve_glyph_and_background_in_one_pass() {
        let _gpu = gpu_test_lock();
        let Some(mut app) = gpu_app() else {
            return;
        };

        let fonts = test_fonts();
        let (cell_w, cell_h) = (fonts.min_width_px() as usize, fonts.height_px() as usize);
        const COLS: usize = 5;

        let entity = {
            let mut images = app.world_mut().resource_mut::<Assets<Image>>();
            let texture_state = TerminalTexture::create(
                COLS as u16,
                1,
                fonts,
                true,
                false,
                [0, 0, 0, 255],
                &mut images,
            )
            .expect("failed to create terminal texture");
            let mut tui = Tui::from_texture_state(texture_state);
            tui.draw(|frame| {
                let buf = frame.buffer_mut();
                buf[(0, 0)].set_symbol("█").set_style(
                    Style::default()
                        .fg(RatatuiColor::Rgb(0, 255, 0))
                        .bg(RatatuiColor::Rgb(255, 0, 0)),
                );
                buf[(1, 0)].set_symbol(" ").set_style(
                    Style::default()
                        .fg(RatatuiColor::Rgb(0, 255, 0))
                        .bg(RatatuiColor::Rgb(0, 0, 255)),
                );
                buf[(2, 0)].set_symbol("M").set_style(
                    Style::default()
                        .fg(RatatuiColor::Rgb(255, 255, 255))
                        .bg(RatatuiColor::Rgb(0, 0, 0)),
                );
                buf[(3, 0)].set_symbol("i\u{331}").set_style(
                    Style::default()
                        .fg(RatatuiColor::Rgb(255, 255, 0))
                        .bg(RatatuiColor::Rgb(255, 0, 255)),
                );
                buf[(4, 0)].set_symbol(" ");
            });
            app.world_mut().spawn(tui).id()
        };

        let pixels = read_back(&mut app, entity);
        assert_eq!(pixels.len(), COLS * cell_w * cell_h * 4);
        let cell = |col: usize| -> Vec<[u8; 4]> {
            (0..cell_h)
                .flat_map(|y| (0..cell_w).map(move |x| (x, y)))
                .map(|(x, y)| {
                    let i = (y * COLS * cell_w + col * cell_w + x) * 4;
                    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
                })
                .collect()
        };

        assert!(
            cell(0).iter().all(|px| *px == [0, 255, 0, 255]),
            "a full-block glyph must be pure foreground"
        );
        assert!(
            cell(1).iter().all(|px| *px == [0, 0, 255, 255]),
            "a space must be pure background"
        );
        let letter = cell(2);
        assert!(letter.contains(&[255, 255, 255, 255]));
        assert!(letter.contains(&[0, 0, 0, 255]));

        let stacked = cell(3);
        let is_fg = |px: &[u8; 4]| px[1] > 128;
        let is_bg = |px: &[u8; 4]| *px == [255, 0, 255, 255];
        assert!(
            stacked.iter().any(is_fg) && stacked.iter().any(is_bg),
            "expected glyph and background pixels, got {stacked:?}"
        );
        let rows_with_fg: Vec<usize> = (0..cell_h)
            .filter(|y| stacked[y * cell_w..(y + 1) * cell_w].iter().any(is_fg))
            .collect();
        assert!(
            rows_with_fg.first().unwrap() + cell_h / 3 < *rows_with_fg.last().unwrap(),
            "both stacked glyphs must be visible, got foreground rows {rows_with_fg:?}"
        );

        assert!(cell(4).iter().all(|px| *px == [0, 0, 0, 255]));
    }

    #[test]
    fn flush_renders_partial_redraw_correctly() {
        let _gpu = gpu_test_lock();
        use crate::bevy_plugin::TerminalPlugin;
        use crate::bevy_plugin::TuiReadbackChannel;
        use bevy::render::renderer::RenderDevice;

        let mut app = App::new();
        app.add_plugins((
            bevy::app::TaskPoolPlugin::default(),
            bevy::asset::AssetPlugin::default(),
            bevy::window::WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            },
            bevy::mesh::MeshPlugin,
            bevy::diagnostic::FrameCountPlugin,
            bevy::time::TimePlugin,
            super::test_render_plugin(),
            bevy::image::ImagePlugin::default(),
            TerminalPlugin::display_only(),
        ));
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

        let paint = |frame: &mut ratatui::Frame, row1_color: RatatuiColor| {
            frame.render_widget(
                Block::default().style(Style::default().bg(RatatuiColor::Red)),
                ratatui::layout::Rect::new(0, 0, 4, 1),
            );
            frame.render_widget(
                Block::default().style(Style::default().bg(row1_color)),
                ratatui::layout::Rect::new(0, 1, 4, 1),
            );
        };

        let entity = {
            let mut images = app.world_mut().resource_mut::<Assets<Image>>();
            let texture_state = TerminalTexture::create(
                4,
                2,
                test_fonts(),
                false,
                false,
                [0, 0, 0, 255],
                &mut images,
            )
            .expect("failed to create terminal texture");
            let mut tui = Tui::from_texture_state(texture_state);
            tui.draw(|frame| paint(frame, RatatuiColor::Green));
            app.world_mut().spawn(tui).id()
        };

        let width_px = app.world().get::<Tui>(entity).unwrap().size_px().x as usize;
        let height_px = app.world().get::<Tui>(entity).unwrap().size_px().y as usize;
        let row0_bytes_len = width_px * 4 * (height_px / 2);

        let image_id = app.world().get::<Tui>(entity).unwrap().image_handle().id();
        let channel = app.world().resource::<TuiReadbackChannel>().clone();

        let request_and_wait = |app: &mut App, channel: &TuiReadbackChannel| -> Vec<u8> {
            let channel = channel.clone();
            let readback = std::thread::spawn(move || channel.request_blocking(image_id));
            for _ in 0..200 {
                app.update();
                if readback.is_finished() {
                    return readback.join().expect("readback thread panicked");
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            panic!("readback never completed - render world stalled");
        };

        let pixels_before = request_and_wait(&mut app, &channel);

        app.world_mut()
            .get_mut::<Tui>(entity)
            .unwrap()
            .draw(|frame| paint(frame, RatatuiColor::Blue));

        let pixels_after = request_and_wait(&mut app, &channel);

        assert_eq!(
            pixels_before[..row0_bytes_len],
            pixels_after[..row0_bytes_len],
            "row 0's pixels must be untouched by a redraw that only changed row 1 - \
             a LoadOp::Clear bug here would wipe them instead of preserving them"
        );

        let row1_before = &pixels_before[row0_bytes_len..];
        let row1_after = &pixels_after[row0_bytes_len..];
        assert!(
            row1_before
                .as_chunks::<4>()
                .0
                .iter()
                .any(|px| px[0..3] == [0, 128, 0]),
            "row 1 must start out green"
        );
        assert!(
            row1_after
                .as_chunks::<4>()
                .0
                .iter()
                .any(|px| px[0..3] == [0, 0, 255]),
            "row 1 must show blue after being redrawn"
        );
    }

    #[cfg(feature = "emoji")]
    mod emoji_gpu {
        use super::*;

        const COLRV1: &[u8] = include_bytes!("backend/testdata/noto_colrv1_subset.ttf");
        const CBDT: &[u8] = include_bytes!("backend/testdata/noto_cbdt_subset.ttf");
        const FIXTURES: [&[u8]; 2] = [COLRV1, CBDT];

        const COLS: usize = 6;
        const BG: [u8; 3] = [10, 20, 200];
        const MAGENTA: RatatuiColor = RatatuiColor::Rgb(255, 0, 255);

        fn emoji_fonts(emoji: &'static [u8]) -> Arc<Fonts> {
            let mut fonts = Fonts::new(
                Font::new(include_bytes!(
                    "../examples/assets/fonts/Mplus1Code-Regular.ttf"
                ))
                .expect("M+ parses"),
                16,
            );
            fonts.add_emoji_fonts([Font::new(emoji).expect("emoji fixture parses")]);
            Arc::new(fonts)
        }

        fn spawn(
            app: &mut App,
            fonts: Arc<Fonts>,
            transparent: bool,
            paint: impl FnOnce(&mut ratatui::buffer::Buffer),
        ) -> Entity {
            let mut images = app.world_mut().resource_mut::<Assets<Image>>();
            let texture_state = TerminalTexture::create(
                COLS as u16,
                3,
                fonts,
                false,
                transparent,
                [0, 0, 0, 255],
                &mut images,
            )
            .expect("failed to create terminal texture");
            let mut tui = Tui::from_texture_state(texture_state);
            tui.draw(|frame| paint(frame.buffer_mut()));
            app.world_mut().spawn(tui).id()
        }

        fn paint(buf: &mut ratatui::buffer::Buffer) {
            let blue = Style::default()
                .fg(RatatuiColor::White)
                .bg(RatatuiColor::Rgb(BG[0], BG[1], BG[2]));
            buf.set_string(0, 0, "a", Style::default());
            buf.set_string(1, 0, "\u{1F600}", blue);
            buf.set_string(3, 0, "b", Style::default());
            buf.set_string(0, 1, "\u{1F600}\u{301}", Style::default().fg(MAGENTA));
            buf.set_string(0, 2, "\u{1F600}", Style::default().fg(MAGENTA));
        }

        fn block(pixels: &[u8], fonts: &Fonts, col: usize, row: usize, w: usize) -> Vec<[u8; 4]> {
            let (cw, ch) = (fonts.min_width_px() as usize, fonts.height_px() as usize);
            (0..ch)
                .flat_map(|y| (0..w * cw).map(move |x| (x, y)))
                .map(|(x, y)| {
                    let i = ((row * ch + y) * COLS * cw + col * cw + x) * 4;
                    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
                })
                .collect()
        }

        fn is_magenta(px: &[u8; 4]) -> bool {
            px[0] > 150 && px[2] > 120 && px[1] < 110
        }

        #[test]
        fn a_color_glyph_has_its_own_colors_over_the_cell_background() {
            let _gpu = gpu_test_lock();
            for emoji in FIXTURES {
                let Some(mut app) = gpu_app() else {
                    return;
                };
                let fonts = emoji_fonts(emoji);
                let entity = spawn(&mut app, Arc::clone(&fonts), false, paint);
                let pixels = read_back(&mut app, entity);
                let (cw, ch) = (fonts.min_width_px() as usize, fonts.height_px() as usize);
                assert_eq!(pixels.len(), COLS * cw * 3 * ch * 4);

                let face = block(&pixels, &fonts, 1, 0, 2);
                assert!(
                    face.iter().any(|px| {
                        let max = px[0].max(px[1]).max(px[2]);
                        let min = px[0].min(px[1]).min(px[2]);
                        max - min > 60
                    }),
                    "an emoji has pixels whose R, G and B differ"
                );
                let bg = [BG[0], BG[1], BG[2], 255];
                for corner in [0, 2 * cw - 1, (ch - 1) * 2 * cw, ch * 2 * cw - 1] {
                    assert_eq!(face[corner], bg, "corner {corner}");
                }

                for col in [0, 3] {
                    let letter = block(&pixels, &fonts, col, 0, 1);
                    assert!(letter.iter().any(|px| px[1] > 200), "col {col}: ink");
                    assert!(letter.contains(&[0, 0, 0, 255]), "col {col}: background");
                }
            }
        }

        #[test]
        fn a_combining_mark_is_drawn_in_the_text_color_on_top_of_the_emoji() {
            let _gpu = gpu_test_lock();
            for emoji in FIXTURES {
                let Some(mut app) = gpu_app() else {
                    return;
                };
                let fonts = emoji_fonts(emoji);
                let entity = spawn(&mut app, Arc::clone(&fonts), false, paint);
                let pixels = read_back(&mut app, entity);

                let plain = block(&pixels, &fonts, 0, 2, 2);
                let marked = block(&pixels, &fonts, 0, 1, 2);
                assert!(
                    !plain.iter().any(is_magenta),
                    "the emoji alone has no magenta"
                );
                assert!(
                    marked.iter().any(is_magenta),
                    "the mark is in the fg color, so the overlay draw is back on the R8 atlas"
                );
                let differing = |px: &&[u8; 4]| px[0].abs_diff(px[2]) > 60;
                assert!(marked.iter().filter(differing).count() > 20);
            }
        }

        #[test]
        fn a_transparent_reset_background_shows_at_the_glyph_corners() {
            let _gpu = gpu_test_lock();
            for emoji in FIXTURES {
                let Some(mut app) = gpu_app() else {
                    return;
                };
                let fonts = emoji_fonts(emoji);
                let entity = spawn(&mut app, Arc::clone(&fonts), true, |buf| {
                    buf.set_string(0, 0, "\u{1F600}", Style::default());
                });
                let pixels = read_back(&mut app, entity);
                let (cw, ch) = (fonts.min_width_px() as usize, fonts.height_px() as usize);
                let face = block(&pixels, &fonts, 0, 0, 2);
                for corner in [0, 2 * cw - 1, (ch - 1) * 2 * cw, ch * 2 * cw - 1] {
                    assert_eq!(face[corner][3], 0, "corner {corner}");
                }
                assert!(face.iter().any(|px| px[3] == 255), "the face is opaque");
            }
        }

        #[test]
        fn a_partial_redraw_leaves_the_emoji_of_other_rows_intact() {
            let _gpu = gpu_test_lock();
            for emoji in FIXTURES {
                let Some(mut app) = gpu_app() else {
                    return;
                };
                let fonts = emoji_fonts(emoji);
                let entity = spawn(&mut app, Arc::clone(&fonts), false, paint);
                let before = read_back(&mut app, entity);

                app.world_mut()
                    .get_mut::<Tui>(entity)
                    .unwrap()
                    .draw(|frame| {
                        paint(frame.buffer_mut());
                        frame.buffer_mut().set_string(0, 2, "xy", Style::default());
                    });
                let after = read_back(&mut app, entity);

                let row_bytes =
                    fonts.height_px() as usize * COLS * fonts.min_width_px() as usize * 4;
                assert_eq!(before[..2 * row_bytes], after[..2 * row_bytes]);
                assert_ne!(before[2 * row_bytes..], after[2 * row_bytes..]);
                assert!(
                    block(&after, &fonts, 1, 0, 2)
                        .iter()
                        .any(|px| px[0].abs_diff(px[2]) > 60),
                    "the emoji is still drawn"
                );
            }
        }
    }
}

#[cfg(test)]
mod tui_request_tests {
    use super::*;
    use crate::bevy_plugin::TerminalPlugin;

    #[test]
    fn asset_font_request_materializes_once_loaded() {
        let mut app = App::new();
        app.add_plugins((
            bevy::app::TaskPoolPlugin::default(),
            bevy::asset::AssetPlugin {
                file_path: "examples/assets".into(),
                ..default()
            },
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
        let texture = TerminalTexture::create(
            4,
            2,
            test_fonts(),
            false,
            false,
            [0, 0, 0, 255],
            &mut images,
        )
        .expect("failed to create terminal texture");
        let original_handle = texture.image_handle();
        let mut tui = Tui::from_texture_state(texture);

        tui.request_resize(8, 6);
        let applied = tui.apply_pending_resize(&mut images);
        assert_eq!(
            applied,
            Some((8, 6)),
            "resize must report the new grid size"
        );
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
        let texture = TerminalTexture::create(
            4,
            2,
            test_fonts(),
            false,
            false,
            [0, 0, 0, 255],
            &mut images,
        )
        .expect("failed to create terminal texture");
        let mut tui = Tui::from_texture_state(texture);

        tui.request_resize(4, 2);
        assert!(
            tui.apply_pending_resize(&mut images).is_none(),
            "requesting the current grid size must not queue a resize"
        );
    }
}

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
        regions.add(0u64, Rect::new(0, 0, 10, 10));
        regions.add(1u64, Rect::new(5, 5, 10, 10));

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
        regions.add(0u64, Rect::new(0, 0, 10, 10));
        regions.add(99u64, Rect::new(0, 0, 10, 10));

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

#[cfg(test)]
mod perf_bench {
    use super::*;
    use crate::fonts::{Font, Fonts};
    use ratatui::style::{Color as RColor, Style};
    use std::time::{Duration, Instant};

    const COLS: u16 = 120;
    const ROWS: u16 = 40;
    const FRAMES: u32 = 600;
    const TEXT: &[u8] = b"The quick brown fox jumps over the lazy dog. 0123456789 +-*/=<>[]{}() ";

    fn run(name: &str, mut paint: impl FnMut(u32, &mut ratatui::buffer::Buffer)) {
        for shaping in [crate::fonts::Shaping::Row, crate::fonts::Shaping::Cell] {
            run_with(name, shaping, false, &mut paint);
        }
    }

    fn run_symbols(name: &str, mut paint: impl FnMut(u32, &mut ratatui::buffer::Buffer)) {
        for shaping in [crate::fonts::Shaping::Row, crate::fonts::Shaping::Cell] {
            run_with(name, shaping, true, &mut paint);
        }
    }

    fn run_with(
        name: &str,
        shaping: crate::fonts::Shaping,
        symbol_font: bool,
        paint: &mut impl FnMut(u32, &mut ratatui::buffer::Buffer),
    ) {
        if let Ok(only) = std::env::var("CPU_HOT_PATH") {
            let (workload, mode) = only.split_once(':').unwrap_or((&only, ""));
            if workload != name || (!mode.is_empty() && mode != format!("{shaping:?}")) {
                return;
            }
        }
        let font_data = include_bytes!("../examples/assets/fonts/Mplus1Code-Regular.ttf");
        let mut fonts = if symbol_font {
            let fusion =
                include_bytes!("../examples/assets/fonts/fusion-pixel-10px-monospaced-ja.ttf");
            let mut fonts = Fonts::new(Font::new(fusion).expect("font"), 16);
            fonts.add_regular_fonts([Font::new(font_data).expect("font")]);
            fonts
        } else {
            Fonts::new(Font::new(font_data).expect("font"), 16)
        };
        fonts.set_shaping(shaping);
        let fonts = Arc::new(fonts);
        let mut images = Assets::<Image>::default();
        let texture =
            TerminalTexture::create(COLS, ROWS, fonts, true, false, [0, 0, 0, 255], &mut images)
                .expect("terminal");
        let mut tui = Tui::from_texture_state(texture);

        let mut draw = Duration::ZERO;
        let mut take = Duration::ZERO;
        let frames = std::env::var("CPU_HOT_PATH_FRAMES")
            .ok()
            .and_then(|f| f.parse().ok())
            .unwrap_or(FRAMES);
        for frame_no in 0..frames + 20 {
            let t0 = Instant::now();
            tui.draw(|frame| paint(frame_no, frame.buffer_mut()));
            let t1 = Instant::now();
            tui.flush();
            let payload = tui.take_pending_draw();
            let uploads = tui.take_shared_font_uploads();
            let t2 = Instant::now();
            std::hint::black_box((&payload, &uploads));
            if frame_no >= 20 {
                draw += t1 - t0;
                take += t2 - t1;
            }
        }
        println!(
            "[cpu_hot_path] {name:<12} {shaping:<4?} draw+shape {:>7.1} us  payload {:>6.1} us  total {:>7.1} us/frame",
            draw.as_secs_f64() * 1e6 / frames as f64,
            take.as_secs_f64() * 1e6 / frames as f64,
            (draw + take).as_secs_f64() * 1e6 / frames as f64,
        );
    }

    fn text_at(frame_no: u32, x: u16, y: u16) -> &'static str {
        let i = (x as usize + y as usize * 7 + frame_no as usize) % TEXT.len();
        std::str::from_utf8(&TEXT[i..i + 1]).unwrap()
    }

    #[test]
    #[ignore = "benchmark - run explicitly with --ignored --nocapture"]
    fn cpu_hot_path() {
        run("full_text", |n, buf| {
            for y in 0..ROWS {
                for x in 0..COLS {
                    let c = (x as u32 * 2 + y as u32 * 5 + n) as u8;
                    buf[(x, y)].set_symbol(text_at(n, x, y)).set_style(
                        Style::default()
                            .fg(RColor::Rgb(c, 255 - c, 128))
                            .bg(RColor::Rgb(c / 4, 0, 64 - c / 4)),
                    );
                }
            }
        });
        run("styled_text", |n, buf| {
            use ratatui::style::Modifier;
            const STYLES: [Modifier; 4] = [
                Modifier::empty(),
                Modifier::BOLD,
                Modifier::ITALIC,
                Modifier::BOLD.union(Modifier::ITALIC),
            ];
            for y in 0..ROWS {
                for x in 0..COLS {
                    let c = (x as u32 * 2 + y as u32 * 5 + n) as u8;
                    let style = STYLES[(x as usize / 6 + y as usize) % STYLES.len()];
                    buf[(x, y)].set_symbol(text_at(n, x, y)).set_style(
                        Style::default()
                            .fg(RColor::Rgb(c, 255 - c, 128))
                            .add_modifier(style),
                    );
                }
            }
        });
        run("unique_text", |n, buf| {
            let mut state = n.wrapping_mul(2654435761).wrapping_add(12345);
            for y in 0..ROWS {
                for x in 0..COLS {
                    state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                    let i = 0x21 + (state >> 24) as usize % 94;
                    const ASCII: &str = "                                 !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";
                    buf[(x, y)].set_symbol(&ASCII[i..i + 1]);
                }
            }
        });
        run("gradient", |n, buf| {
            for y in 0..ROWS {
                for x in 0..COLS {
                    let c = (x as u32 * 2 + y as u32 * 5 + n) as u8;
                    buf[(x, y)]
                        .set_symbol("█")
                        .set_style(Style::default().fg(RColor::Rgb(c, 255 - c, 128)));
                }
            }
        });
        run("colors_only", |n, buf| {
            for y in 0..ROWS {
                for x in 0..COLS {
                    let c = (x as u32 * 2 + y as u32 * 5 + n) as u8;
                    buf[(x, y)]
                        .set_symbol(text_at(0, x, y))
                        .set_style(Style::default().fg(RColor::Rgb(c, 255 - c, 128)));
                }
            }
        });
        run("one_row", |n, buf| {
            for y in 0..ROWS {
                for x in 0..COLS {
                    let frame_no = if y == 5 { n } else { 0 };
                    buf[(x, y)].set_symbol(text_at(frame_no, x, y));
                }
            }
        });
        run_symbols("symbols", |n, buf| {
            for y in 0..ROWS {
                let i = (y as u32 + n) as usize;
                let icon = char::from_u32(0xE000 + (i % 14) as u32).unwrap();
                let line = format!("{icon} file_{i:04}.rs → {icon}{icon} size {} ", i * 37);
                for (x, ch) in line
                    .chars()
                    .chain(std::iter::repeat(' '))
                    .take(COLS as usize)
                    .enumerate()
                {
                    buf[(x as u16, y)].set_char(ch);
                }
            }
        });
        run("static", |_, buf| {
            for y in 0..ROWS {
                for x in 0..COLS {
                    buf[(x, y)].set_symbol(text_at(0, x, y));
                }
            }
        });
    }
}
