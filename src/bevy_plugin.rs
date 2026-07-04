// This module provides a Bevy plugin that integrates BevyTerminalBackend
// into Bevy applications.

use bevy::pbr::{Material, StandardMaterial};
use bevy::prelude::*;
use bevy::render::renderer::{RenderDevice, RenderQueue};
use tracing::{info, warn};
use wgpu;

use crate::input::*;
use crate::setup::{Tui, TuiSurface};

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
            info!("Keyboard input enabled");
        }

        #[cfg(feature = "mouse_input")]
        if self.input_config.mouse_enabled {
            app.add_systems(
                Update,
                (update_cursor_position_system, mouse_input_system)
                    .chain()
                    .in_set(TerminalSystemSet::Input),
            );

            info!("Unified mouse input enabled (2D + 3D auto-detection)");
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
            info!("Auto-focus (Tab cycling) enabled");
        }

        // Plugin-owned GPU plumbing for the `Tui` component: collects
        // pending async copies, starts new renders, and touches
        // StandardMaterial - all so user drawing systems can take zero
        // render-resource parameters.
        app.add_systems(Update, gpu_flush_system.in_set(TerminalSystemSet::Render));

        // Attaching a Tui to an existing mesh. Runs early so the same-frame
        // Render pass sees the swapped material.
        app.add_systems(
            Update,
            crate::setup::attach_terminal_system.in_set(TerminalSystemSet::Input),
        );

        info!("TerminalPlugin initialized with input handling");
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

/// Implemented for material types that can display a terminal texture, so
/// [`TerminalMaterialPlugin`] knows how to re-point (or just touch) the
/// material at the `Tui`'s image handle when it changes. This is required
/// because 3D materials do not observe `Image` asset mutations on their own
/// (see CLAUDE.md "Common Gotchas" #1) - `Assets::get_mut` must be called
/// with a real field write to reliably mark the asset changed.
pub trait TerminalMaterial: Material {
    /// Point (or re-point) this material at the terminal's texture.
    fn set_terminal_texture(&mut self, image: &Handle<Image>);
}

impl TerminalMaterial for StandardMaterial {
    fn set_terminal_texture(&mut self, image: &Handle<Image>) {
        self.base_color_texture = Some(image.clone());
    }
}

/// Blanket impl for any `ExtendedMaterial<StandardMaterial, E>` (e.g.
/// retro_crt's `CrtMaterial`). This must live here, in the crate that
/// defines `TerminalMaterial`: `ExtendedMaterial` is a foreign type, so
/// downstream crates cannot write this impl themselves (orphan rules only
/// permit implementing a *local* trait for a foreign type, not the reverse).
/// Because of this blanket impl, users get `TerminalMaterialPlugin` support
/// for any `ExtendedMaterial<StandardMaterial, _>` for free - no manual
/// `TerminalMaterial` impl needed.
impl<E: bevy::pbr::MaterialExtension> TerminalMaterial
    for bevy::pbr::ExtendedMaterial<StandardMaterial, E>
{
    fn set_terminal_texture(&mut self, image: &Handle<Image>) {
        self.base.base_color_texture = Some(image.clone());
    }
}

/// Plugin-owned GPU flush for every [`Tui`](crate::setup::Tui) entity.
/// Registered automatically by [`TerminalPlugin`] in
/// `TerminalSystemSet::Render`. For each `Tui`:
///
/// 1. If a pending async copy exists: poll it; if ready, copy into the
///    `Image` asset and unmap. Runs regardless of whether `draw()` was
///    called this frame, so static content completes without the user
///    calling `draw()` again.
/// 2. If dirty and no pending copy: render to the GPU texture and start a
///    new async copy; clear dirty.
/// 3. If the `Image` was updated this frame, touch `StandardMaterial` for
///    every [`TuiSurface`](crate::setup::TuiSurface) pointing at this `Tui`.
///    This is hardcoded here (not through the generic
///    [`TerminalMaterialPlugin`] mechanism) because `StandardMaterial` is
///    the overwhelmingly common case, and a direct touch avoids the
///    scheduling overhead of a separate per-type system. Custom/extended
///    material types opt in via `TerminalMaterialPlugin::<M>`.
pub fn gpu_flush_system(
    mut terminals: Query<(Entity, &mut Tui)>,
    surfaces: Query<(&TuiSurface, &MeshMaterial3d<StandardMaterial>)>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut images: ResMut<Assets<Image>>,
    mut std_materials: ResMut<Assets<StandardMaterial>>,
) {
    for (entity, mut tui) in &mut terminals {
        let applied = tui.flush(&render_device, &render_queue, &mut images);
        if !applied {
            continue;
        }

        for (surface, material_handle) in &surfaces {
            if surface.tui != entity {
                continue;
            }
            if let Some(mut material) = std_materials.get_mut(&material_handle.0) {
                material.set_terminal_texture(tui.image_handle());
            }
        }
    }
}

/// Generic per-material-type touch system for [`Tui`] surfaces using a
/// custom material (e.g. an `ExtendedMaterial`-based shader). Register once
/// per material type:
///
/// ```ignore
/// app.add_plugins(TerminalMaterialPlugin::<CrtMaterial>::default());
/// ```
///
/// Do not register this for `StandardMaterial` - [`gpu_flush_system`]
/// already handles it directly (see its doc comment for the performance
/// rationale). Doing so anyway is harmless (the asset is simply touched
/// twice); a warning is logged when the plugin is built.
pub struct TerminalMaterialPlugin<M: TerminalMaterial>(std::marker::PhantomData<M>);

impl<M: TerminalMaterial> Default for TerminalMaterialPlugin<M> {
    fn default() -> Self {
        Self(std::marker::PhantomData)
    }
}

impl<M: TerminalMaterial> Plugin for TerminalMaterialPlugin<M> {
    fn build(&self, app: &mut App) {
        if std::any::TypeId::of::<M>() == std::any::TypeId::of::<StandardMaterial>() {
            warn!(
                "TerminalMaterialPlugin::<StandardMaterial> registered explicitly; \
                 gpu_flush_system already touches StandardMaterial directly, so this \
                 will touch it a second time (harmless, just wasted work)."
            );
        }
        app.add_systems(
            Update,
            touch_terminal_material_system::<M>.in_set(TerminalSystemSet::Render),
        );
    }
}

fn touch_terminal_material_system<M: TerminalMaterial>(
    terminals: Query<&Tui>,
    surfaces: Query<(&TuiSurface, &MeshMaterial3d<M>)>,
    mut materials: ResMut<Assets<M>>,
) {
    for (surface, material_handle) in &surfaces {
        let Ok(tui) = terminals.get(surface.tui) else {
            continue;
        };
        if let Some(mut material) = materials.get_mut(&material_handle.0) {
            material.set_terminal_texture(tui.image_handle());
        }
    }
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

