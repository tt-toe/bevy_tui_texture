// This module provides a Bevy plugin that integrates BevyTerminalBackend
// into Bevy applications.

use bevy::prelude::*;
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::sprite_render::{ColorMaterial, MeshMaterial2d};
use ratatui::Terminal;
use tracing::info;
use wgpu;

use crate::BevyTerminalBackend;
use crate::input::*;

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

        info!("TerminalPlugin initialized with input handling");
    }
}

/// Component marker for terminal UI entities.
#[derive(Component)]
pub struct TerminalComponent;

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

/// Resource that holds the terminal instance.
///
/// This resource is initialized during startup and provides access to
/// the terminal for rendering TUI content.
///
/// Note: The backend is owned by the Terminal, access it via terminal.backend_mut()
#[derive(Resource)]
pub struct TerminalResource {
    /// The ratatui Terminal instance with BevyTerminalBackend
    pub terminal: Terminal<BevyTerminalBackend>,

    /// GPU texture for rendering terminal output
    pub texture: wgpu::Texture,

    /// Handle to the Bevy Image for display
    pub image_handle: Handle<Image>,

    /// Texture dimensions
    pub width: u32,
    pub height: u32,
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
    images: &mut ResMut<Assets<Image>>,
) {
    // Copy GPU texture to Bevy Image - do everything INSIDE get_mut scope for change detection
    if let Some(image) = images.get_mut(image_handle) {
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

        render_device.wgpu_device().poll(wgpu::PollType::Wait).ok();

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

/// Complete terminal update: render to GPU, copy to Image, and update material.
///
/// Call after `terminal.draw()` to handle all three update steps in one function.
#[allow(clippy::too_many_arguments)]
pub fn update_terminal_and_material<T: Component>(
    terminal: &mut ratatui::Terminal<BevyTerminalBackend>,
    texture: &wgpu::Texture,
    image_handle: &Handle<Image>,
    width: u32,
    height: u32,
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
    images: &mut ResMut<Assets<Image>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    query: &Query<(&MeshMaterial2d<ColorMaterial>, &T)>,
) {
    // 1. Render to GPU texture
    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    terminal.backend_mut().render_to_texture(
        render_device.wgpu_device(),
        render_queue.0.as_ref(),
        &texture_view,
    );

    // 2. Copy GPU texture to Bevy Image
    update_terminal_texture(
        texture,
        image_handle,
        width,
        height,
        render_device,
        render_queue,
        images,
    );

    // 3. Trigger material change detection
    update_material_texture(materials, query, image_handle);
}

/// Update material texture to trigger Bevy's change detection.
///
/// Generic over marker component type `T` for flexible querying.
pub fn update_material_texture<T: Component>(
    materials: &mut ResMut<Assets<ColorMaterial>>,
    query: &Query<(&MeshMaterial2d<ColorMaterial>, &T)>,
    image_handle: &Handle<Image>,
) {
    for (material_handle, _) in query.iter() {
        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.texture = Some(image_handle.clone());
        }
    }
}

/// Placeholder system for terminal updates. Implement your own with `terminal.draw()`.
pub fn update_terminal_content_system(_terminal_res: ResMut<TerminalResource>) {
    // Placeholder - user should implement their own update logic
}

// ============================================================================
// Helper Functions for Spawning Terminals
// ============================================================================

/// Spawn interactive terminal entity with keyboard and mouse input.
///
/// **Use Case**: Manual [`TerminalTexture`](crate::setup::TerminalTexture) management with automatic entity setup.
/// For full automation, use [`SimpleTerminal2D`](crate::setup::SimpleTerminal2D).
pub fn spawn_interactive_terminal(
    commands: &mut Commands,
    image_handle: Handle<Image>,
    size: Vec2,
    position: Vec3,
) -> Entity {
    commands
        .spawn((
            ImageNode::new(image_handle),
            Node {
                width: Val::Px(size.x),
                height: Val::Px(size.y),
                ..default()
            },
            Transform::from_translation(position),
            TerminalComponent,
            TerminalInput::default(), // Enable both keyboard and mouse
        ))
        .id()
}

/// Spawn display-only terminal without input (for logs, status panels).
///
/// **Use Case**: Manual [`TerminalTexture`](crate::setup::TerminalTexture) for static displays.
/// For full automation, use [`SimpleTerminal2D`](crate::setup::SimpleTerminal2D) with input disabled.
pub fn spawn_display_terminal(
    commands: &mut Commands,
    image_handle: Handle<Image>,
    size: Vec2,
    position: Vec3,
) -> Entity {
    commands
        .spawn((
            ImageNode::new(image_handle),
            Node {
                width: Val::Px(size.x),
                height: Val::Px(size.y),
                ..default()
            },
            Transform::from_translation(position),
            TerminalComponent,
            // No TerminalInput = display-only
        ))
        .id()
}

/// Spawn absolutely-positioned terminal for tiled layouts with z-index support.
///
/// **Use Case**: Manual [`TerminalTexture`](crate::setup::TerminalTexture) with absolute positioning.
/// For full automation, use [`SimpleTerminal2D`](crate::setup::SimpleTerminal2D).
#[allow(clippy::too_many_arguments)]
pub fn spawn_positioned_terminal(
    commands: &mut Commands,
    image_handle: Handle<Image>,
    cols: u16,
    rows: u16,
    char_width_px: u32,
    char_height_px: u32,
    left: f32,
    top: f32,
    z_index: Option<i32>,
    enable_input: bool,
) -> Entity {
    let width = cols as f32 * char_width_px as f32;
    let height = rows as f32 * char_height_px as f32;

    info!(
        "Spawning positioned terminal: {}x{} chars, {}x{} px, pos=({}, {}), z={:?}, input={}",
        cols, rows, width, height, left, top, z_index, enable_input
    );

    let mut entity_commands = commands.spawn((
        ImageNode::new(image_handle),
        Node {
            width: Val::Px(width),
            height: Val::Px(height),
            position_type: bevy::ui::PositionType::Absolute,
            left: Val::Px(left),
            top: Val::Px(top),
            ..default()
        },
        Transform::default(),
        TerminalComponent,
        TerminalDimensions {
            cols,
            rows,
            char_width_px,
            char_height_px,
        },
    ));

    if enable_input {
        entity_commands.insert(TerminalInput::default());
    }

    if let Some(z) = z_index {
        entity_commands.insert(bevy::ui::ZIndex(z));
    }

    entity_commands.id()
}
