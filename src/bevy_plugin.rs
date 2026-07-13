// This module provides a Bevy plugin that integrates BevyTerminalBackend
// into Bevy applications.

use bevy::prelude::*;
use bevy::render::render_asset::RenderAssets;
use bevy::render::renderer::{render_system, RenderDevice, RenderQueue};
use bevy::render::texture::GpuImage;
use bevy::render::{ExtractSchedule, MainWorld, Render, RenderApp, RenderSystems};
use std::collections::HashMap;
use tracing::debug;
use wgpu;

use crate::backend::SharedFontGpuState;
use crate::backend::TerminalDrawPayload;
use crate::backend::TerminalGpuState;
use crate::input::*;
use crate::setup::Tui;

/// System sets for organizing terminal systems.
///
/// Execution order: Input → UserUpdate → Render
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum TerminalSystemSet {
    /// Input capture systems (runs early)
    Input,
    /// User update systems (runs after Input)
    UserUpdate,
    /// Rendering systems (runs late)
    Render,
}

/// Bevy plugin for terminal rendering and input handling.
///
/// Use `TerminalPlugin::default()` for full input, `TerminalPlugin::display_only()` for no input,
/// or `TerminalPlugin::new(config)` for custom configuration.
#[derive(Default)]
pub struct TerminalPlugin {
    /// Configuration for input handling
    pub input_config: TerminalInputConfig,
}

impl TerminalPlugin {
    /// Create a new TerminalPlugin with custom input configuration.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use bevy::prelude::*;
    /// use bevy_tui_texture::prelude::*;
    ///
    /// let plugin = TerminalPlugin::new(TerminalInputConfig {
    ///     keyboard_enabled: true,
    ///     mouse_enabled: true,
    ///     auto_focus: true,
    ///     focus_button: MouseButton::Left,
    /// });
    /// ```
    pub fn new(config: TerminalInputConfig) -> Self {
        Self {
            input_config: config,
        }
    }

    /// Create a plugin with keyboard input disabled.
    ///
    /// Mouse input and auto-focus remain enabled.
    pub fn without_keyboard() -> Self {
        Self {
            input_config: TerminalInputConfig {
                keyboard_enabled: false,
                ..Default::default()
            },
        }
    }

    /// Create a plugin with mouse input disabled.
    ///
    /// Keyboard input and auto-focus remain enabled.
    pub fn without_mouse() -> Self {
        Self {
            input_config: TerminalInputConfig {
                mouse_enabled: false,
                ..Default::default()
            },
        }
    }

    /// Create a plugin with all input handling disabled (display-only mode).
    ///
    /// Terminals will render but not respond to input events.
    /// Useful for static displays like logs or status panels.
    pub fn display_only() -> Self {
        Self {
            input_config: TerminalInputConfig {
                keyboard_enabled: false,
                mouse_enabled: false,
                auto_focus: false,
                ..Default::default()
            },
        }
    }
}

impl Plugin for TerminalPlugin {
    fn build(&self, app: &mut App) {
        // Fonts loadable via the AssetServer.
        app.init_asset::<crate::fonts::TerminalFontAsset>()
            .init_asset_loader::<crate::fonts::TerminalFontAssetLoader>();

        // Register messages (events)
        app.add_message::<TerminalEvent>();

        // Insert resources
        app.insert_resource(self.input_config.clone());
        app.insert_resource(TerminalFocus::default());
        app.insert_resource(CursorPosition::default());

        // Configure system sets with execution order
        app.configure_sets(
            Update,
            (
                TerminalSystemSet::Input,
                TerminalSystemSet::UserUpdate,
                TerminalSystemSet::Render,
            )
                .chain(), // Run in order: Input → UserUpdate → Render
        );

        // Register input systems (conditionally based on config and features)
        #[cfg(feature = "keyboard_input")]
        if self.input_config.keyboard_enabled {
            app.add_systems(
                Update,
                keyboard_input_system.in_set(TerminalSystemSet::Input),
            );
            debug!("Keyboard input enabled");
        }

        #[cfg(feature = "mouse_input")]
        if self.input_config.mouse_enabled {
            app.add_systems(
                Update,
                (update_cursor_position_system, mouse_input_system)
                    .chain()
                    .in_set(TerminalSystemSet::Input),
            );

            debug!("Unified mouse input enabled (2D + 3D auto-detection)");
        }

        // Window resize system (always enabled)
        app.add_systems(
            Update,
            window_resize_system.in_set(TerminalSystemSet::Input),
        );

        if self.input_config.auto_focus {
            app.add_systems(
                Update,
                terminal_focus_system.in_set(TerminalSystemSet::Input),
            );
            debug!("Auto-focus (Tab cycling) enabled");
        }

        // Declarative spawning: turn `TuiRequest` components into live
        // terminals. Scheduled before the Input set so a terminal
        // materialized this frame is already visible to the same frame's
        // input and user-draw systems.
        app.add_systems(
            Update,
            crate::setup::materialize_tui_requests.before(TerminalSystemSet::Input),
        );

        // Plugin-owned CPU-side plumbing for the `Tui` component: extracts a
        // draw payload from dirty terminals, so user drawing systems can
        // take zero render-resource parameters. The actual GPU render
        // happens in the render world - see `extract_tui_draws` /
        // `render_tui_textures` below.
        app.add_systems(Update, gpu_flush_system.in_set(TerminalSystemSet::Render));
        #[cfg(feature = "3d")]
        app.add_systems(
            Update,
            resize_world_quad_meshes
                .after(gpu_flush_system)
                .in_set(TerminalSystemSet::Render),
        );

        // Attaching a Tui to an existing mesh. Runs early so the same-frame
        // Render pass sees the swapped material.
        #[cfg(feature = "3d")]
        app.add_systems(
            Update,
            crate::setup::attach_terminal_system.in_set(TerminalSystemSet::Input),
        );

        // Main-world half of the blocking-readback channel (see
        // `Tui::read_back_blocking`). Always inserted, even without a
        // render sub-app - `TuiReadbackChannel::request_blocking` degrades
        // to returning an empty buffer if nothing is listening.
        let (readback_tx, readback_rx) = std::sync::mpsc::channel();
        app.insert_resource(TuiReadbackChannel(readback_tx));

        // Render-world half of the GPU render: extract each dirty
        // terminal's CPU-computed draw payload, then render it directly
        // into the destination `GpuImage`'s own texture. Absent (no render
        // sub-app) in configurations without a rendering backend, e.g. some
        // headless test setups - silently skip registration there, matching
        // every other bevy render-world plugin's own convention.
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .init_resource::<PendingTuiDraws>()
                .init_resource::<TerminalGpuStore>()
                .init_resource::<SharedFontGpuStore>()
                .init_resource::<PendingFontUploads>()
                .init_resource::<LiveFontKeys>()
                .init_resource::<PendingTuiReadbacks>()
                .insert_resource(TuiReadbackReceiver(std::sync::Mutex::new(readback_rx)))
                .add_systems(ExtractSchedule, extract_tui_draws)
                .add_systems(
                    Render,
                    (render_tui_textures, process_tui_readbacks)
                        .chain()
                        .in_set(RenderSystems::Render)
                        // Ensures this frame's terminal-texture submit lands
                        // before bevy's own main-pass submit (inside
                        // `render_system`), so materials sample this
                        // frame's content instead of the previous frame's -
                        // otherwise wgpu's submit-order execution makes the
                        // display lag one frame behind `Tui::draw()`.
                        .before(render_system),
                );
        }

        debug!("TerminalPlugin initialized with input handling");
    }
}

/// Terminal dimensions component.
///
/// Stores the actual terminal grid dimensions (columns and rows) and font metrics
/// for accurate mouse coordinate conversion.
#[derive(Component, Debug, Clone, Copy)]
pub struct TerminalDimensions {
    pub cols: u16,
    pub rows: u16,
    pub char_width_px: u32,
    pub char_height_px: u32,
}

// ============================================================================
// `Tui` GPU plumbing
// ============================================================================

/// Plugin-owned CPU flush for every [`Tui`] entity. Registered automatically
/// by [`TerminalPlugin`] in `TerminalSystemSet::Render`. If dirty, extracts
/// a `TerminalDrawPayload` from the backend and stashes it for
/// `extract_tui_draws` to pick up; see [`Tui::flush`](crate::setup::Tui) for
/// details. Pure CPU work - no `RenderDevice`/`RenderQueue` needed at all,
/// the actual GPU render happens in the render world (`render_tui_textures`,
/// below), which writes directly into the exact texture the destination
/// material's bind group already references - no material touching
/// anywhere.
pub fn gpu_flush_system(
    mut terminals: Query<(&mut Tui, Option<&mut TerminalDimensions>)>,
    mut images: ResMut<Assets<Image>>,
) {
    for (mut tui, dimensions) in &mut terminals {
        if let (Some((cols, rows)), Some(mut dimensions)) =
            (tui.apply_pending_resize(&mut images), dimensions)
        {
            let size = tui.size_px();
            dimensions.cols = cols;
            dimensions.rows = rows;
            dimensions.char_width_px = size.x / (cols.max(1) as u32);
            dimensions.char_height_px = size.y / (rows.max(1) as u32);
        }
        tui.flush();
    }
}

/// Recomputes a [`TuiKind::WorldQuad`](crate::setup::TuiKind::WorldQuad)
/// terminal's mesh aspect ratio after a resize. Registered automatically by
/// `TerminalPlugin` (feature `3d`), after `gpu_flush_system` in the same
/// `TerminalSystemSet::Render` - keyed on `Changed<TerminalDimensions>`
/// rather than a separate signal, since `gpu_flush_system` only writes that
/// component when a resize actually happened.
#[cfg(feature = "3d")]
fn resize_world_quad_meshes(
    mut terminals: Query<
        (&TerminalDimensions, &crate::setup::WorldQuadHeight, &mut Mesh3d),
        Changed<TerminalDimensions>,
    >,
    // `Option`: only exists once something registers it (bevy's PbrPlugin,
    // normally) - a headless or 2D-only app shouldn't fail this system's
    // parameter validation over a resource `TuiKind::WorldQuad` alone needs
    // (same reasoning as `materialize_tui_requests`).
    meshes: Option<ResMut<Assets<Mesh>>>,
) {
    let Some(mut meshes) = meshes else {
        return;
    };
    for (dimensions, height, mut mesh3d) in &mut terminals {
        let aspect = dimensions.cols as f32 * dimensions.char_width_px as f32
            / (dimensions.rows as f32 * dimensions.char_height_px as f32);
        let half_height = height.0 / 2.0;
        mesh3d.0 = meshes.add(Plane3d::new(
            Vec3::Z,
            Vec2::new(half_height * aspect, half_height),
        ));
    }
}

// ============================================================================
// Render-world GPU render (replaces the old GPU->GPU copy entirely)
// ============================================================================

/// Render-world resource: draw payloads extracted this frame (or a prior
/// frame, if the destination `GpuImage` wasn't ready yet), keyed by
/// destination `Image` asset id, still waiting to be rendered.
#[derive(Resource, Default)]
struct PendingTuiDraws(HashMap<AssetId<Image>, TerminalDrawPayload>);

/// Render-world resource: the GPU pipelines/atlas texture for each terminal,
/// keyed by destination `Image` asset id. Created lazily on a terminal's
/// first render (`render_tui_textures`) and evicted automatically once the
/// destination image itself is gone (see that function's doc comment).
#[derive(Resource, Default)]
struct TerminalGpuStore(HashMap<AssetId<Image>, TerminalGpuState>);

/// Render-world resource: per-font glyph-atlas texture + compositor
/// pipelines (IMPROVEMENT.md C3), keyed by [`crate::fonts::Fonts::identity`]
/// rather than by destination image - terminals that share a `Fonts` share
/// one entry here. Created lazily in `render_tui_textures`, evicted once no
/// live `Tui` reports that font key anymore (see [`LiveFontKeys`]).
#[derive(Resource, Default)]
struct SharedFontGpuStore(HashMap<usize, SharedFontGpuState>);

/// Render-world resource: glyph rasterizations queued by [`extract_tui_draws`]
/// for each font, still waiting to be uploaded to that font's shared atlas.
/// Multiple terminals sharing a font may all contribute here in the same
/// frame (only the first one visited by `Fonts::with_shared_cpu_state`
/// actually drains anything non-empty; see
/// `Tui::take_shared_font_uploads`), so entries are appended to rather than
/// overwritten. Drained (per font, once uploaded) by `render_tui_textures`.
#[derive(Resource, Default)]
struct PendingFontUploads(HashMap<usize, Vec<(crate::utils::text_atlas::CacheRect, Vec<u32>)>>);

/// Render-world resource: every font key currently reported by a live `Tui`
/// entity, recomputed from scratch each extract. Used by `render_tui_textures`
/// to evict [`SharedFontGpuStore`] entries for fonts no longer in use by any
/// terminal - unlike [`TerminalGpuStore`]'s per-destination eviction (which
/// piggybacks on `RenderAssets<GpuImage>` naturally dropping a despawned
/// terminal's image), a shared font's last user despawning leaves no asset
/// to check against, so it needs this explicit liveness set instead.
#[derive(Resource, Default)]
struct LiveFontKeys(std::collections::HashSet<usize>);

/// Extract system: drains each `Tui`'s pending draw payload (set by
/// [`gpu_flush_system`] via [`Tui::flush`](crate::setup::Tui::flush)) into
/// the render-world [`PendingTuiDraws`] map, and (IMPROVEMENT.md C3) each
/// `Tui`'s font key + any pending shared-atlas glyph uploads into
/// [`LiveFontKeys`]/[`PendingFontUploads`]. Runs in the render world but
/// mutates the main world through [`MainWorld`] (rather than the read-only
/// `Extract<Query>>`) because draining the payload - so a static terminal's
/// next frame doesn't re-push the same draw - requires `&mut Tui`.
fn extract_tui_draws(
    mut main_world: ResMut<MainWorld>,
    mut pending: ResMut<PendingTuiDraws>,
    mut font_uploads: ResMut<PendingFontUploads>,
    mut live_fonts: ResMut<LiveFontKeys>,
    mut query_state: Local<Option<QueryState<&'static mut Tui>>>,
) {
    // Cache the `QueryState` across frames (IMPROVEMENT.md D2) instead of
    // constructing a fresh one (archetype matching from scratch) every
    // frame - `QueryState::iter_mut` updates its archetype caches
    // internally on every call, so no explicit `update_archetypes` is
    // needed here.
    let query = query_state.get_or_insert_with(|| main_world.query());
    live_fonts.0.clear();
    for mut tui in query.iter_mut(&mut main_world) {
        let (font_key, uploads) = tui.take_shared_font_uploads();
        live_fonts.0.insert(font_key);
        if !uploads.is_empty() {
            font_uploads.0.entry(font_key).or_default().extend(uploads);
        }

        if let Some((dest, draw)) = tui.take_pending_draw() {
            // Simply replaces any entry still in the map from a frame
            // `render_tui_textures` skipped (destination `GpuImage` not
            // prepared yet) - safe to drop that older payload's vertex
            // data outright, since `draw` is this frame's full correct
            // geometry regardless. (Before IMPROVEMENT.md C3 moved glyph
            // rasterizations out of this payload and into the shared
            // per-font state, an older payload's one-shot atlas uploads
            // had to be merged forward here or those glyphs would render
            // as garbage forever - no longer a concern for this map.)
            pending.0.insert(dest, draw);
        }
    }
}

/// Render-world system: renders every pending draw payload queued by
/// [`extract_tui_draws`] directly into its destination `GpuImage`'s own
/// texture, in [`RenderSystems::Render`] (after
/// [`RenderSystems::PrepareAssets`], so newly created destination
/// `GpuImage`s are already prepared). A destination not yet prepared (can
/// happen on a terminal's very first frame) is left in the map and retried
/// next frame - it is never dropped.
///
/// Eviction: [`TerminalGpuStore`] entries are retained only for destination
/// images still present in `RenderAssets<GpuImage>` - once a `Tui`
/// despawns, its last strong `Handle<Image>` drops, bevy's own asset
/// extraction removes the `GpuImage`, and the next call here drops this
/// store's now-orphaned GPU state (atlas texture, pipelines) right along
/// with it. No entity-level bookkeeping needed.
///
/// Batches every terminal's draw into one `CommandEncoder` and submits it
/// once at the end (IMPROVEMENT.md B3), instead of each terminal creating
/// its own encoder and submitting separately. Skipped entirely on a frame
/// with nothing pending, so a fully static scene still submits nothing.
#[allow(clippy::too_many_arguments)]
fn render_tui_textures(
    mut pending: ResMut<PendingTuiDraws>,
    mut store: ResMut<TerminalGpuStore>,
    mut font_store: ResMut<SharedFontGpuStore>,
    mut font_uploads: ResMut<PendingFontUploads>,
    live_fonts: Res<LiveFontKeys>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
) {
    // A font with no live `Tui` left using it (its last terminal despawned)
    // has no destination image to key eviction off of the way
    // `TerminalGpuStore` does below, hence the separate liveness set.
    font_store.0.retain(|key, _| live_fonts.0.contains(key));

    if pending.0.is_empty() {
        store.0.retain(|dest, _| gpu_images.get(*dest).is_some());
        font_uploads.0.clear();
        return;
    }

    let mut encoder =
        render_device
            .wgpu_device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Terminal Draw Encoder (batched)"),
            });

    pending.0.retain(|dest, draw| {
        let Some(gpu_image) = gpu_images.get(*dest) else {
            return true; // destination GpuImage not prepared yet - retry next frame
        };

        let shared = font_store.0.entry(draw.font_key()).or_insert_with(|| {
            SharedFontGpuState::new(
                render_device.wgpu_device(),
                render_queue.0.as_ref(),
                gpu_image.texture_descriptor.format,
            )
        });
        if let Some(uploads) = font_uploads.0.remove(&draw.font_key()) {
            shared.upload_glyphs(render_queue.0.as_ref(), &uploads);
        }

        let gpu_state = store
            .0
            .entry(*dest)
            .or_insert_with(|| TerminalGpuState::new(render_device.wgpu_device(), shared));
        gpu_state.render(
            render_device.wgpu_device(),
            render_queue.0.as_ref(),
            shared,
            &mut encoder,
            &gpu_image.texture_view,
            draw,
        );
        false // rendered - drop from the pending map
    });

    render_queue.0.submit(Some(encoder.finish()));

    store.0.retain(|dest, _| gpu_images.get(*dest).is_some());
    font_uploads.0.clear();
}

// ============================================================================
// Blocking CPU readback (goes through the render world via a channel)
// ============================================================================

/// One outstanding readback request: which destination image to read, and
/// where to send the resulting tightly-packed RGBA8 bytes.
struct TuiReadbackRequest {
    image_id: AssetId<Image>,
    response: std::sync::mpsc::Sender<Vec<u8>>,
}

/// Main-world resource: the sending half of the blocking-readback channel.
/// Cloned into [`Tui::read_back_blocking`](crate::setup::Tui::read_back_blocking)
/// callers. Always present (inserted by [`TerminalPlugin`] regardless of
/// whether a render sub-app exists) - if nothing is listening on the other
/// end, `request_blocking` degrades to returning an empty buffer rather
/// than blocking forever.
#[derive(Resource, Clone)]
pub struct TuiReadbackChannel(std::sync::mpsc::Sender<TuiReadbackRequest>);

impl TuiReadbackChannel {
    /// Blocks the calling thread until the render world has read the
    /// destination image's pixels back, or returns an empty `Vec` if no
    /// render world is listening. Call this from a thread other than the
    /// one driving `App::update()` (as with `PipelinedRenderingPlugin`,
    /// rendering happens on its own thread) - calling it from the same
    /// thread that must also advance the render world deadlocks.
    pub(crate) fn request_blocking(&self, image_id: AssetId<Image>) -> Vec<u8> {
        let (tx, rx) = std::sync::mpsc::channel();
        if self
            .0
            .send(TuiReadbackRequest {
                image_id,
                response: tx,
            })
            .is_err()
        {
            return Vec::new(); // no render world listening
        }
        rx.recv().unwrap_or_default()
    }
}

/// Render-world resource: the receiving half of the blocking-readback
/// channel, drained every frame by [`process_tui_readbacks`]. Wrapped in a
/// `Mutex` solely to satisfy `Resource`'s `Sync` bound - `mpsc::Receiver`
/// itself is never actually contended (only this one system ever locks it).
#[derive(Resource)]
struct TuiReadbackReceiver(std::sync::Mutex<std::sync::mpsc::Receiver<TuiReadbackRequest>>);

/// Render-world resource: requests whose destination `GpuImage` wasn't
/// prepared yet, retried next frame - the same "not ready yet" pattern as
/// [`PendingTuiDraws`].
#[derive(Resource, Default)]
struct PendingTuiReadbacks(Vec<TuiReadbackRequest>);

/// Render-world system: drains newly arrived readback requests plus any
/// retried from a prior frame, and for each whose destination `GpuImage` is
/// prepared, performs a synchronous GPU->CPU copy and sends the result back
/// over the request's response channel.
fn process_tui_readbacks(
    receiver: Res<TuiReadbackReceiver>,
    mut pending: ResMut<PendingTuiReadbacks>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
) {
    pending.0.extend(receiver.0.lock().unwrap().try_iter());
    if pending.0.is_empty() {
        return;
    }

    pending.0.retain(|request| {
        let Some(gpu_image) = gpu_images.get(request.image_id) else {
            return true; // destination GpuImage not prepared yet - retry next frame
        };

        let width = gpu_image.texture_descriptor.size.width;
        let height = gpu_image.texture_descriptor.size.height;
        let pixels = read_back_gpu_image_blocking(
            &gpu_image.texture,
            width,
            height,
            &render_device,
            &render_queue,
        );
        request.response.send(pixels).ok();
        false // handled - drop from the pending list
    });
}

/// Round `unpadded_bytes_per_row` up to wgpu's required row alignment
/// (`COPY_BYTES_PER_ROW_ALIGNMENT`, 256 bytes) - `wgpu::Texture`->buffer
/// copies require each row to start at an aligned offset, so a texture
/// whose tightly-packed row size isn't already a multiple of that alignment
/// needs padding bytes appended to every row in the staging buffer. Pure
/// arithmetic, no wgpu resources touched.
fn padded_bytes_per_row(unpadded_bytes_per_row: u32) -> u32 {
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padding = (align - (unpadded_bytes_per_row % align)) % align;
    unpadded_bytes_per_row + padding
}

/// Strip wgpu's row padding from a GPU-readback buffer, copying row by row
/// into a tightly-packed `Vec<u8>` of exactly `unpadded_bytes_per_row *
/// height` bytes. Pure function - `padded` is already-mapped buffer memory,
/// no wgpu calls made here.
fn strip_row_padding(
    padded: &[u8],
    unpadded_bytes_per_row: u32,
    bytes_per_row: u32,
    height: u32,
) -> Vec<u8> {
    let mut out = vec![0u8; (unpadded_bytes_per_row * height) as usize];
    if bytes_per_row == unpadded_bytes_per_row {
        out.copy_from_slice(padded);
    } else {
        for y in 0..height {
            let src_offset = (y * bytes_per_row) as usize;
            let dst_offset = (y * unpadded_bytes_per_row) as usize;
            out[dst_offset..dst_offset + unpadded_bytes_per_row as usize].copy_from_slice(
                &padded[src_offset..src_offset + unpadded_bytes_per_row as usize],
            );
        }
    }
    out
}

/// Blocking GPU->CPU copy of a whole texture's RGBA8 pixels, with wgpu row
/// padding already stripped. Shared by [`process_tui_readbacks`].
fn read_back_gpu_image_blocking(
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
) -> Vec<u8> {
    let unpadded_bytes_per_row = width * 4;
    let bytes_per_row = padded_bytes_per_row(unpadded_bytes_per_row);
    let buffer_size = (bytes_per_row * height) as wgpu::BufferAddress;

    let staging_buffer = render_device
        .wgpu_device()
        .create_buffer(&wgpu::BufferDescriptor {
            label: Some("Terminal Readback Buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

    let mut encoder = render_device
        .wgpu_device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Terminal Readback Encoder"),
        });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &staging_buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
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
    let out = strip_row_padding(&mapped, unpadded_bytes_per_row, bytes_per_row, height);
    drop(mapped);
    staging_buffer.unmap();
    out
}

// ============================================================================
// Test: staging-row padding math (P2-4). Pure CPU - no wgpu device needed
// (the readback opt-in still uses this math, kept even after Phase A/B).
// ============================================================================

#[cfg(test)]
mod row_padding_tests {
    use super::*;

    #[test]
    fn already_aligned_rows_need_no_padding() {
        // COPY_BYTES_PER_ROW_ALIGNMENT is 256; 256 and 512 are both already
        // multiples of it.
        assert_eq!(padded_bytes_per_row(256), 256);
        assert_eq!(padded_bytes_per_row(512), 512);
    }

    #[test]
    fn unaligned_rows_round_up_to_the_next_multiple() {
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        // A 10px-wide RGBA8 row is 40 bytes - far under one alignment unit,
        // so it must round up to exactly one unit.
        assert_eq!(padded_bytes_per_row(40), align);
        // One byte over an aligned size must round up to the NEXT multiple,
        // not stay at the same one.
        assert_eq!(padded_bytes_per_row(align + 1), align * 2);
    }

    #[test]
    fn zero_width_needs_no_padding() {
        assert_eq!(padded_bytes_per_row(0), 0);
    }

    #[test]
    fn strip_row_padding_is_a_no_op_when_already_unpadded() {
        let data: Vec<u8> = (0..16).collect(); // 4 rows x 4 bytes, unpadded
        let out = strip_row_padding(&data, 4, 4, 4);
        assert_eq!(out, data);
    }

    #[test]
    fn strip_row_padding_removes_padding_bytes_between_rows() {
        // 2 rows, 4 real bytes each, padded to 6 bytes/row (2 padding bytes
        // per row that must be dropped, not just have their content kept).
        #[rustfmt::skip]
        let padded: Vec<u8> = vec![
            1, 2, 3, 4, 0xAA, 0xAA, // row 0: 4 real bytes + 2 padding bytes
            5, 6, 7, 8, 0xAA, 0xAA, // row 1: 4 real bytes + 2 padding bytes
        ];
        let out = strip_row_padding(&padded, 4, 6, 2);
        assert_eq!(out, vec![1, 2, 3, 4, 5, 6, 7, 8], "padding bytes must be dropped, real bytes kept in row order");
    }

    #[test]
    fn strip_row_padding_handles_a_single_row() {
        let padded = vec![9, 9, 9, 9, 0, 0, 0, 0]; // 1 row, 4 real + 4 padding
        let out = strip_row_padding(&padded, 4, 8, 1);
        assert_eq!(out, vec![9, 9, 9, 9]);
    }
}

