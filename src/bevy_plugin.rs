use bevy::prelude::*;
use bevy::render::render_asset::RenderAssets;
use bevy::render::renderer::{
    FlushCommands, RenderContext, RenderDevice, RenderGraph, RenderGraphSystems, RenderQueue,
};
use bevy::render::texture::GpuImage;
use bevy::render::{ExtractSchedule, MainWorld, RenderApp};
use std::collections::HashMap;
use tracing::debug;
use wgpu;

use crate::backend::SharedFontGpuState;
use crate::backend::TerminalDrawPayload;
use crate::backend::TerminalGpuState;
use crate::input::*;
use crate::setup::Tui;

/// System sets for organizing terminal systems.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum TerminalSystemSet {
    /// Input capture systems (runs early)
    Input,
    /// User update systems (runs after Input)
    UserUpdate,
    /// Rendering systems (runs late).
    Render,
}

/// Bevy plugin for terminal rendering and input handling.
#[derive(Default)]
pub struct TerminalPlugin {
    /// Configuration for input handling
    pub input_config: TerminalInputConfig,
}

impl TerminalPlugin {
    /// Create a new TerminalPlugin with custom input configuration.
    pub fn new(config: TerminalInputConfig) -> Self {
        Self {
            input_config: config,
        }
    }

    /// Create a plugin with keyboard input disabled.
    pub fn without_keyboard() -> Self {
        Self {
            input_config: TerminalInputConfig {
                keyboard_enabled: false,
                ..Default::default()
            },
        }
    }

    /// Create a plugin with mouse input disabled.
    pub fn without_mouse() -> Self {
        Self {
            input_config: TerminalInputConfig {
                mouse_enabled: false,
                ..Default::default()
            },
        }
    }

    /// Create a plugin with all input handling disabled (display-only mode).
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
        app.init_asset::<crate::fonts::TerminalFontAsset>()
            .init_asset_loader::<crate::fonts::TerminalFontAssetLoader>();

        app.add_message::<TerminalEvent>();

        app.insert_resource(self.input_config.clone());
        app.insert_resource(TerminalFocus::default());
        app.insert_resource(CursorPosition::default());

        app.configure_sets(
            Update,
            (
                TerminalSystemSet::Input,
                TerminalSystemSet::UserUpdate,
                TerminalSystemSet::Render,
            )
                .chain(),
        );

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

        app.add_systems(
            Update,
            crate::setup::materialize_tui_requests.before(TerminalSystemSet::Input),
        );

        #[cfg(feature = "blink")]
        app.add_systems(
            Update,
            blink_system
                .after(TerminalSystemSet::Render)
                .before(gpu_flush_system),
        );
        app.add_systems(Update, gpu_flush_system.after(TerminalSystemSet::Render));
        #[cfg(feature = "3d")]
        app.add_systems(Update, resize_world_quad_meshes.after(gpu_flush_system));

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

        // The `RenderGraphSystems` sets are only weakly chained (ordered
        // only where ECS access conflicts), and command buffers and the GPU
        // are invisible to that, so every ordering is explicit: terminals
        // render before the camera passes, are submitted after, and the
        // blocking readback runs after the submit. No render sub-app (some
        // headless setups) means nothing to register.
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            #[cfg(feature = "emoji")]
            render_app.init_resource::<PendingColorUploads>();
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
                    RenderGraph,
                    (
                        render_tui_textures
                            .in_set(RenderGraphSystems::Begin)
                            .before(RenderGraphSystems::Render),
                        // For apps without `CorePipelinePlugin`, whose own
                        // submit normally does this; with it, whichever runs
                        // second finds nothing pending.
                        flush_tui_commands
                            .in_set(RenderGraphSystems::Submit)
                            .after(render_tui_textures)
                            .after(RenderGraphSystems::Render),
                        process_tui_readbacks
                            .in_set(RenderGraphSystems::Finish)
                            .after(flush_tui_commands),
                    ),
                );
        }

        debug!("TerminalPlugin initialized with input handling");
    }
}

/// Terminal dimensions component.
#[derive(Component, Debug, Clone, Copy)]
#[component(summary_tick)]
pub struct TerminalDimensions {
    pub cols: u16,
    pub rows: u16,
    pub char_width_px: u32,
    pub char_height_px: u32,
}

/// Switches blinking rows between their lit and blank halves on a real-time clock.
#[cfg(feature = "blink")]
fn blink_system(time: Option<Res<Time<Real>>>, mut terminals: Query<&mut Tui>) {
    let Some(time) = time else {
        return;
    };
    let off = crate::backend::blink_off_at(time.elapsed());
    for mut tui in &mut terminals {
        // Shared access first: a mutable deref would flag the terminal as changed.
        if tui.blink_needs_update(off) {
            tui.set_blink_off(off);
        }
    }
}

/// Plugin-owned CPU flush for every [`Tui`] entity.
pub fn gpu_flush_system(
    mut terminals: Query<(&mut Tui, Option<&mut TerminalDimensions>)>,
    mut images: ResMut<Assets<Image>>,
) {
    for (mut tui, dimensions) in &mut terminals {
        // Checked through shared access first: a terminal with nothing to
        // do must not be mutably dereferenced (nor `Assets<Image>`), which
        // would flag it as changed every single frame.
        if tui.has_pending_resize()
            && let (Some((cols, rows)), Some(mut dimensions)) =
                (tui.apply_pending_resize(&mut images), dimensions)
        {
            let size = tui.size_px();
            dimensions.cols = cols;
            dimensions.rows = rows;
            dimensions.char_width_px = size.x / (cols.max(1) as u32);
            dimensions.char_height_px = size.y / (rows.max(1) as u32);
        }
        if tui.needs_flush() {
            tui.flush();
        }
    }
}

/// Recomputes a [`TuiKind::WorldQuad`](crate::setup::TuiKind::WorldQuad)
/// terminal's mesh aspect ratio after a resize.
#[cfg(feature = "3d")]
fn resize_world_quad_meshes(
    mut terminals: Query<
        (
            &TerminalDimensions,
            &crate::setup::WorldQuadHeight,
            &mut Mesh3d,
        ),
        Changed<TerminalDimensions>,
    >,
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

/// Render-world resource: draw payloads extracted this frame (or a prior
/// frame, if the destination `GpuImage` wasn't ready yet), keyed by
/// destination `Image` asset id, still waiting to be rendered.
#[derive(Resource, Default)]
struct PendingTuiDraws(HashMap<AssetId<Image>, TerminalDrawPayload>);

/// Render-world resource: the GPU pipelines/atlas texture for each terminal,
/// keyed by destination `Image` asset id.
#[derive(Resource, Default)]
struct TerminalGpuStore(HashMap<AssetId<Image>, TerminalGpuState>);

/// Render-world resource: per-font glyph-atlas texture + compositor
/// pipelines, keyed by [`crate::fonts::Fonts::identity`]
/// rather than by destination image - terminals that share a `Fonts` share
/// one entry here.
#[derive(Resource, Default)]
struct SharedFontGpuStore(HashMap<usize, SharedFontGpuState>);

/// Render-world resource: glyph rasterizations queued by [`extract_tui_draws`]
/// for each font, still waiting to be uploaded to that font's shared atlas.
#[derive(Resource, Default)]
struct PendingFontUploads(HashMap<usize, Vec<(crate::utils::text_atlas::CacheRect, Vec<u8>)>>);

/// [`PendingFontUploads`] for the color glyph atlas: per font, the atlas's
/// texture size and its queued rasterizations (premultiplied RGBA8).
#[cfg(feature = "emoji")]
#[derive(Resource, Default)]
struct PendingColorUploads(
    HashMap<
        usize,
        (
            (u32, u32),
            Vec<(crate::utils::text_atlas::CacheRect, Vec<u8>)>,
        ),
    >,
);

/// Render-world resource: every font key currently reported by a live `Tui`
/// entity, recomputed from scratch each extract.
#[derive(Resource, Default)]
struct LiveFontKeys(std::collections::HashSet<usize>);

/// Extract system: drains each `Tui`'s pending draw payload (set by
/// [`gpu_flush_system`] via [`Tui::flush`](crate::setup::Tui::flush)) into
/// the render-world [`PendingTuiDraws`] map, and each
/// `Tui`'s font key + any pending shared-atlas glyph uploads into
/// [`LiveFontKeys`]/[`PendingFontUploads`].
fn extract_tui_draws(
    mut main_world: ResMut<MainWorld>,
    mut pending: ResMut<PendingTuiDraws>,
    mut font_uploads: ResMut<PendingFontUploads>,
    #[cfg(feature = "emoji")] mut color_uploads: ResMut<PendingColorUploads>,
    mut live_fonts: ResMut<LiveFontKeys>,
    mut query_state: Local<Option<QueryState<&'static mut Tui>>>,
) {
    let query = query_state.get_or_insert_with(|| main_world.query());
    live_fonts.0.clear();
    for mut tui in query.iter_mut(&mut main_world) {
        let (font_key, uploads) = tui.take_shared_font_uploads();
        live_fonts.0.insert(font_key);
        if !uploads.is_empty() {
            font_uploads.0.entry(font_key).or_default().extend(uploads);
        }
        #[cfg(feature = "emoji")]
        {
            let (size, uploads) = tui.take_shared_color_uploads();
            if !uploads.is_empty() {
                color_uploads
                    .0
                    .entry(font_key)
                    .or_insert_with(|| (size, Vec::new()))
                    .1
                    .extend(uploads);
            }
        }

        if pending.0.contains_key(&tui.image_handle().id()) {
            continue;
        }

        if tui.has_pending_draw()
            && let Some((dest, draw)) = tui.take_pending_draw()
        {
            pending.0.insert(dest, draw);
        }
    }
}

/// Render-world system: renders every pending draw payload queued by
/// [`extract_tui_draws`] directly into its destination `GpuImage`'s own
/// texture, in [`RenderGraphSystems::Begin`] - structurally before this
/// same frame's camera passes (`RenderGraphSystems::Render`), which is
/// what gives terminal updates same-frame (not one-frame-lagged) display.
#[allow(clippy::too_many_arguments)]
fn render_tui_textures(
    mut pending: ResMut<PendingTuiDraws>,
    mut store: ResMut<TerminalGpuStore>,
    mut font_store: ResMut<SharedFontGpuStore>,
    mut font_uploads: ResMut<PendingFontUploads>,
    #[cfg(feature = "emoji")] mut color_uploads: ResMut<PendingColorUploads>,
    live_fonts: Res<LiveFontKeys>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut render_context: RenderContext,
) {
    font_store.0.retain(|key, _| live_fonts.0.contains(key));

    // Uploads can arrive in a frame with no draw for their font, and the CPU
    // atlas already marks them cached: upload to existing atlases and keep
    // the rest until their font's GPU state exists, never drop them.
    for (key, uploads) in &mut font_uploads.0 {
        if let Some(shared) = font_store.0.get(key) {
            shared.upload_glyphs(&render_queue, uploads);
            uploads.clear();
        }
    }
    font_uploads
        .0
        .retain(|key, uploads| !uploads.is_empty() && live_fonts.0.contains(key));
    #[cfg(feature = "emoji")]
    {
        for (key, (size, uploads)) in &mut color_uploads.0 {
            if let Some(shared) = font_store.0.get_mut(key) {
                shared.upload_color_glyphs(
                    render_device.wgpu_device(),
                    &render_queue,
                    *size,
                    uploads,
                );
                uploads.clear();
            }
        }
        color_uploads
            .0
            .retain(|key, (_, uploads)| !uploads.is_empty() && live_fonts.0.contains(key));
    }

    if pending.0.is_empty() {
        store.0.retain(|dest, _| gpu_images.get(*dest).is_some());
        return;
    }

    let encoder = render_context.command_encoder();

    pending.0.retain(|dest, draw| {
        let Some(gpu_image) = gpu_images.get(*dest) else {
            return true;
        };

        let shared = font_store.0.entry(draw.font_key()).or_insert_with(|| {
            SharedFontGpuState::new(
                render_device.wgpu_device(),
                &render_queue,
                gpu_image.texture_descriptor.format,
            )
        });
        if let Some(uploads) = font_uploads.0.remove(&draw.font_key()) {
            shared.upload_glyphs(&render_queue, &uploads);
        }
        #[cfg(feature = "emoji")]
        if let Some((size, uploads)) = color_uploads.0.remove(&draw.font_key()) {
            shared.upload_color_glyphs(render_device.wgpu_device(), &render_queue, size, &uploads);
        }

        let gpu_state = store
            .0
            .entry(*dest)
            .or_insert_with(|| TerminalGpuState::new(render_device.wgpu_device(), shared));
        gpu_state.render(
            render_device.wgpu_device(),
            &render_queue,
            shared,
            encoder,
            &gpu_image.texture_view,
            draw,
        );
        false
    });

    store.0.retain(|dest, _| gpu_images.get(*dest).is_some());
}

/// Submits every command buffer [`render_tui_textures`] recorded via
/// [`RenderContext`] this frame, exactly like bevy_core_pipeline's own
/// `submit_pending_command_buffers` (which normally does this - both take
/// `ResMut<PendingCommandBuffers>`, so whichever runs first submits
/// everything pending in recording order and the other is an empty no-op,
/// never a double submit).
fn flush_tui_commands(mut flush: FlushCommands) {
    flush.flush();
}

/// One outstanding readback request: which destination image to read, and
/// where to send the resulting tightly-packed RGBA8 bytes.
struct TuiReadbackRequest {
    image_id: AssetId<Image>,
    response: std::sync::mpsc::Sender<Vec<u8>>,
}

/// Main-world resource: the sending half of the blocking-readback channel.
#[derive(Resource, Clone)]
pub struct TuiReadbackChannel(std::sync::mpsc::Sender<TuiReadbackRequest>);

impl TuiReadbackChannel {
    /// Blocks the calling thread until the render world has read the
    /// destination image's pixels back, or returns an empty `Vec` if no
    /// render world is listening.
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
            return Vec::new();
        }
        rx.recv().unwrap_or_default()
    }
}

/// Render-world resource: the receiving half of the blocking-readback
/// channel, drained every frame by [`process_tui_readbacks`].
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
    // A poisoned lock only means another thread panicked mid-send; the
    // queued requests are still valid.
    pending.0.extend(
        receiver
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .try_iter(),
    );
    if pending.0.is_empty() {
        return;
    }

    pending.0.retain(|request| {
        let Some(gpu_image) = gpu_images.get(request.image_id) else {
            return true;
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
        false
    });
}

/// Round `unpadded_bytes_per_row` up to wgpu's required row alignment
/// (`COPY_BYTES_PER_ROW_ALIGNMENT`, 256 bytes) - `wgpu::Texture`->buffer
/// copies require each row to start at an aligned offset, so a texture
/// whose tightly-packed row size isn't already a multiple of that alignment
/// needs padding bytes appended to every row in the staging buffer.
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
    let row = unpadded_bytes_per_row as usize;
    let mut out = vec![0u8; row * height as usize];
    for (y, dst) in out.chunks_exact_mut(row.max(1)).enumerate() {
        let src_offset = y * bytes_per_row as usize;
        if let Some(src) = padded.get(src_offset..src_offset + dst.len()) {
            dst.copy_from_slice(src);
        }
    }
    out
}

/// Blocking GPU->CPU copy of a whole texture's RGBA8 pixels, with wgpu row
/// padding already stripped.
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

    let mut encoder =
        render_device
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
    render_queue.submit(Some(encoder.finish()));

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
    if !matches!(receiver.recv(), Ok(Ok(()))) {
        tracing::warn!("Tui readback: mapping the staging buffer failed");
        return Vec::new();
    }
    let Ok(mapped) = buffer_slice.get_mapped_range() else {
        tracing::warn!("Tui readback: the staging buffer is not mapped");
        return Vec::new();
    };
    let out = strip_row_padding(&mapped, unpadded_bytes_per_row, bytes_per_row, height);
    drop(mapped);
    staging_buffer.unmap();
    out
}

#[cfg(test)]
mod row_padding_tests {
    use super::*;

    #[test]
    fn a_short_readback_buffer_leaves_missing_rows_zeroed() {
        let out = strip_row_padding(&[1, 2, 3, 4, 9, 9], 4, 6, 2);
        assert_eq!(out, [1, 2, 3, 4, 0, 0, 0, 0]);
    }

    #[test]
    fn already_aligned_rows_need_no_padding() {
        assert_eq!(padded_bytes_per_row(256), 256);
        assert_eq!(padded_bytes_per_row(512), 512);
    }

    #[test]
    fn unaligned_rows_round_up_to_the_next_multiple() {
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        assert_eq!(padded_bytes_per_row(40), align);
        assert_eq!(padded_bytes_per_row(align + 1), align * 2);
    }

    #[test]
    fn zero_width_needs_no_padding() {
        assert_eq!(padded_bytes_per_row(0), 0);
    }

    #[test]
    fn strip_row_padding_is_a_no_op_when_already_unpadded() {
        let data: Vec<u8> = (0..16).collect();
        let out = strip_row_padding(&data, 4, 4, 4);
        assert_eq!(out, data);
    }

    #[test]
    fn strip_row_padding_removes_padding_bytes_between_rows() {
        #[rustfmt::skip]
        let padded: Vec<u8> = vec![
            1, 2, 3, 4, 0xAA, 0xAA,
            5, 6, 7, 8, 0xAA, 0xAA,
        ];
        let out = strip_row_padding(&padded, 4, 6, 2);
        assert_eq!(
            out,
            vec![1, 2, 3, 4, 5, 6, 7, 8],
            "padding bytes must be dropped, real bytes kept in row order"
        );
    }

    #[test]
    fn strip_row_padding_handles_a_single_row() {
        let padded = vec![9, 9, 9, 9, 0, 0, 0, 0];
        let out = strip_row_padding(&padded, 4, 8, 1);
        assert_eq!(out, vec![9, 9, 9, 9]);
    }
}
