// This module provides a Bevy plugin that integrates BevyTerminalBackend
// into Bevy applications.

use bevy::prelude::*;
use bevy::render::render_asset::RenderAssets;
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::texture::GpuImage;
use bevy::render::{ExtractSchedule, MainWorld, Render, RenderApp, RenderSystems};
use std::collections::HashMap;
use tracing::debug;
use wgpu;

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

        // Plugin-owned GPU plumbing for the `Tui` component: renders dirty
        // terminals into their library-owned texture, so user drawing
        // systems can take zero render-resource parameters. The actual
        // `Image`/`GpuImage` update happens in the render world - see
        // `extract_tui_copies` / `copy_tui_textures` below.
        app.add_systems(Update, gpu_flush_system.in_set(TerminalSystemSet::Render));

        // Attaching a Tui to an existing mesh. Runs early so the same-frame
        // Render pass sees the swapped material.
        #[cfg(feature = "3d")]
        app.add_systems(
            Update,
            crate::setup::attach_terminal_system.in_set(TerminalSystemSet::Input),
        );

        // Render-world half of the GPU->GPU copy: extract which terminals
        // were re-rendered this frame, then copy their library-owned
        // texture into the destination `GpuImage`. Absent (no render
        // sub-app) in configurations without a rendering backend, e.g. some
        // headless test setups - silently skip registration there, matching
        // every other bevy render-world plugin's own convention.
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .init_resource::<PendingTuiCopies>()
                .add_systems(ExtractSchedule, extract_tui_copies)
                .add_systems(Render, copy_tui_textures.in_set(RenderSystems::Render));
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

/// Plugin-owned GPU flush for every [`Tui`] entity. Registered automatically
/// by [`TerminalPlugin`] in `TerminalSystemSet::Render`. If dirty, renders
/// into the library-owned texture and marks a copy pending; see
/// [`Tui::flush`](crate::setup::Tui) for details. No material touching
/// happens here or anywhere else - the render-world copy
/// (`copy_tui_textures`, below) writes directly into the exact texture the
/// destination material's bind group already references, so there is no
/// asset mutation to react to and nothing to re-touch.
pub fn gpu_flush_system(
    mut terminals: Query<&mut Tui>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
) {
    for mut tui in &mut terminals {
        tui.flush(&render_device, &render_queue);
    }
}

// ============================================================================
// Render-world GPU->GPU copy (replaces the old CPU readback entirely)
// ============================================================================

/// One outstanding texture-to-texture copy, keyed by destination `Image`
/// asset id in [`PendingTuiCopies`]. Persists across render frames until
/// performed - the sole retry mechanism for "destination `GpuImage` not
/// prepared yet" (see `copy_tui_textures`).
struct PendingTuiCopy {
    source: wgpu::Texture,
    size: wgpu::Extent3d,
}

/// Render-world resource: terminals whose library-owned texture was
/// re-rendered this frame (or a prior frame, if the destination `GpuImage`
/// wasn't ready yet) and still need their copy performed.
#[derive(Resource, Default)]
struct PendingTuiCopies(HashMap<AssetId<Image>, PendingTuiCopy>);

/// Extract system: drains each `Tui`'s pending-copy flag (set by
/// [`gpu_flush_system`] via [`Tui::flush`](crate::setup::Tui::flush)) into
/// the render-world [`PendingTuiCopies`] map. Runs in the render world but
/// mutates the main world through [`MainWorld`] (rather than the read-only
/// `Extract<Query>>`) because clearing the flag - so a static terminal's
/// next frame doesn't re-push the same copy - requires `&mut Tui`.
fn extract_tui_copies(mut main_world: ResMut<MainWorld>, mut pending: ResMut<PendingTuiCopies>) {
    let mut query = main_world.query::<&mut Tui>();
    for mut tui in query.iter_mut(&mut main_world) {
        if let Some((source, dest, size)) = tui.take_copy_pending() {
            pending.0.insert(dest, PendingTuiCopy { source, size });
        }
    }
}

/// Render-world system: performs every pending GPU->GPU copy queued by
/// [`extract_tui_copies`], in [`RenderSystems::Render`] (after
/// [`RenderSystems::PrepareAssets`], so newly created destination
/// `GpuImage`s are already prepared). A destination not yet prepared (can
/// happen on a terminal's very first frame) is left in the map and retried
/// next frame - it is never dropped.
fn copy_tui_textures(
    mut pending: ResMut<PendingTuiCopies>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
) {
    if pending.0.is_empty() {
        return;
    }

    let mut encoder = render_device
        .wgpu_device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Tui GPU->GPU Copy Encoder"),
        });

    pending.0.retain(|dest, copy| {
        let Some(gpu_image) = gpu_images.get(*dest) else {
            return true; // destination GpuImage not prepared yet - retry next frame
        };
        encoder.copy_texture_to_texture(
            copy.source.as_image_copy(),
            gpu_image.texture.as_image_copy(),
            copy.size,
        );
        false // performed - drop from the pending map
    });

    render_queue.0.submit(Some(encoder.finish()));
}

/// Copy GPU texture to Bevy Image with proper padding alignment.
///
/// Call after `terminal.backend_mut().render_to_texture()` to update the Bevy Image asset.
pub fn update_terminal_texture(
    texture: &wgpu::Texture,
    image_handle: &Handle<Image>,
    width: u32,
    height: u32,
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
    images: &mut Assets<Image>,
) {
    // Copy GPU texture to Bevy Image - do everything INSIDE get_mut scope for change detection
    // (bevy 0.19: get_mut returns an AssetMut guard, hence `mut`)
    if let Some(mut image) = images.get_mut(image_handle) {
        let unpadded_bytes_per_row = width * 4;
        let bytes_per_row = {
            let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
            let padding = (align - (unpadded_bytes_per_row % align)) % align;
            unpadded_bytes_per_row + padding
        };

        let buffer_size = (bytes_per_row * height) as wgpu::BufferAddress;

        let staging_buffer = render_device
            .wgpu_device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("Terminal Staging Buffer"),
                size: buffer_size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });

        let mut encoder =
            render_device
                .wgpu_device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Terminal Copy Encoder"),
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

        render_device.wgpu_device().poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        }).ok();

        if receiver.recv().ok().and_then(|r| r.ok()).is_some() {
            let data = buffer_slice.get_mapped_range();

            if let Some(image_data) = &mut image.data {
                // Copy row by row, skipping padding
                if bytes_per_row == unpadded_bytes_per_row {
                    // No padding, direct copy
                    image_data.copy_from_slice(&data);
                } else {
                    // Has padding, copy row by row
                    for y in 0..height {
                        let src_offset = (y * bytes_per_row) as usize;
                        let dst_offset = (y * unpadded_bytes_per_row) as usize;
                        let row_data =
                            &data[src_offset..src_offset + unpadded_bytes_per_row as usize];
                        image_data[dst_offset..dst_offset + unpadded_bytes_per_row as usize]
                            .copy_from_slice(row_data);
                    }
                }
            }
        }

        staging_buffer.unmap();
    }
}

