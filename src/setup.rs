//! Simplified terminal API for quick setup and prototyping.
//!
//! This module provides three levels of abstraction for creating and managing terminals:
//!
//! 1. **`TerminalTexture`** - Core texture operations only
//!    - Just creates the terminal texture and ratatui Terminal
//!    - User must manually spawn entities and add input components
//!    - Maximum flexibility and control
//!
//! 2. **`SimpleTerminal2D`** - Full 2D setup
//!    - Automatically creates texture, spawns entity with 2D components
//!    - Handles input setup based on flags
//!    - Perfect for 2D UI overlays and HUDs
//!
//! 3. **`SimpleTerminal3D`** - Full 3D setup
//!    - Automatically creates texture, spawns 3D mesh entity
//!    - Supports position, rotation, and scale in 3D space
//!    - Perfect for in-game terminals and spatial UIs
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
//!         TerminalComponent,
//!         texture.dimensions(),
//!     ));
//! }
//! ```
//!
//! ## Level 2: SimpleTerminal2D (Full 2D Setup)
//!
//! ```ignore
//! use bevy::prelude::*;
//! use bevy_tui_texture::setup::SimpleTerminal2D;
//!
//! fn setup(/* ... */) {
//!     let terminal = SimpleTerminal2D::create_and_spawn(
//!         80, 25, fonts, (0.0, 0.0), true, true, true,
//!         &mut commands, &render_device, &render_queue, &mut images,
//!     ).unwrap();
//! }
//! ```
//!
//! ## Level 3: SimpleTerminal3D (Full 3D Setup)
//!
//! ```ignore
//! use bevy::prelude::*;
//! use bevy_tui_texture::setup::SimpleTerminal3D;
//!
//! fn setup(/* ... */) {
//!     let terminal = SimpleTerminal3D::create_and_spawn(
//!         80, 25, fonts,
//!         Vec3::ZERO, Quat::IDENTITY, Vec3::ONE,
//!         MyMarker,
//!         true, true, true,
//!         &mut commands, &mut meshes, &mut materials,
//!         &render_device, &render_queue, &mut images,
//!     ).unwrap();
//! }
//! ```

use std::sync::Arc;
use std::sync::Mutex;

use bevy::asset::RenderAssetUsages;
use bevy::pbr::StandardMaterial;
use bevy::prelude::*;
use bevy::render::renderer::{RenderDevice, RenderQueue};
use wgpu::Buffer;

use crate::backend::bevy_backend::{BevyTerminalBackend, TerminalBuilder};
use crate::bevy_plugin::{TerminalComponent, TerminalDimensions};
use crate::fonts::Fonts;
use crate::input::TerminalInput;

/// Async GPU→CPU buffer copy state
///
/// Manages a staging buffer with async mapping for non-blocking GPU texture readback.
/// This enables 1-frame latency texture updates without blocking the CPU.
struct AsyncCopy {
    buffer: Buffer,
    ready: Arc<Mutex<Option<Result<(), wgpu::BufferAsyncError>>>>,
    height: u32,
    bytes_per_row: u32,
    unpadded_bytes_per_row: u32,
}

impl AsyncCopy {
    /// Check if the buffer mapping is complete (non-blocking)
    fn is_ready(&self) -> bool {
        self.ready.lock().unwrap().is_some()
    }

    /// Copy buffer contents to an image (call only after is_ready() returns true)
    fn copy_to_image(&self, image: &mut Image) {
        let buffer_slice = self.buffer.slice(..);
        let data = buffer_slice.get_mapped_range();

        if let Some(image_data) = &mut image.data {
            if self.bytes_per_row == self.unpadded_bytes_per_row {
                // No padding, direct copy
                image_data.copy_from_slice(&data);
            } else {
                // Has padding, copy row by row
                for y in 0..self.height {
                    let src_offset = (y * self.bytes_per_row) as usize;
                    let dst_offset = (y * self.unpadded_bytes_per_row) as usize;
                    let row_data =
                        &data[src_offset..src_offset + self.unpadded_bytes_per_row as usize];
                    image_data[dst_offset..dst_offset + self.unpadded_bytes_per_row as usize]
                        .copy_from_slice(row_data);
                }
            }
        }
    }

    /// Create a new async copy from texture to staging buffer
    fn from_texture(
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
        render_device: &RenderDevice,
        render_queue: &RenderQueue,
    ) -> Self {
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
                label: Some("Terminal Staging Buffer (Async)"),
                size: buffer_size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });

        let mut encoder =
            render_device
                .wgpu_device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Terminal Copy Encoder (Async)"),
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

        // Issue async map request
        let buffer_slice = staging_buffer.slice(..);
        let ready = Arc::new(Mutex::new(None));
        let ready_clone = Arc::clone(&ready);

        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            *ready_clone.lock().unwrap() = Some(result);
        });

        Self {
            buffer: staging_buffer,
            ready,
            height,
            bytes_per_row,
            unpadded_bytes_per_row,
        }
    }
}

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
    pending_copy: Option<AsyncCopy>, // Async buffer copy in-flight
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
        images: &mut ResMut<Assets<Image>>,
    ) -> Result<Self, String> {
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

        // Create Bevy Image with white background (will be immediately overwritten)
        let mut image = Image::new_fill(
            bevy::render::render_resource::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            &[255, 255, 255, 255], // White instead of black to debug
            bevy::render::render_resource::TextureFormat::Rgba8Unorm,
            default(), // Default render asset usages
        );
        image.texture_descriptor.usage = bevy::render::render_resource::TextureUsages::COPY_DST
            | bevy::render::render_resource::TextureUsages::TEXTURE_BINDING;
        let image_handle = images.add(image);

        // Create backend
        let mut backend = TerminalBuilder::new(fonts)
            .with_dimensions(cols, rows)
            .build(render_device.wgpu_device(), render_queue.0.as_ref())
            .map_err(|e| format!("Failed to build backend: {:?}", e))?;

        // Optionally pre-populate programmatic glyphs
        if programmatic_glyphs {
            backend
                .populate_programmatic_glyphs(render_queue.0.as_ref())
                .map_err(|e| format!("Failed to populate glyphs: {:?}", e))?;
        }

        let terminal = ratatui::Terminal::new(backend)
            .map_err(|e| format!("Failed to create terminal: {}", e))?;

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
            pending_copy: None,
        })
    }

    /// Update the terminal texture with new content.
    ///
    /// This method:
    /// 1. Calls the provided drawing function with a ratatui Frame
    /// 2. Renders the terminal to the GPU texture
    /// 3. Copies the GPU texture to the Bevy Image
    ///
    /// # Arguments
    ///
    /// * `render_device` - Bevy's RenderDevice resource
    /// * `render_queue` - Bevy's RenderQueue resource
    /// * `images` - Bevy's Image assets
    /// * `draw_fn` - Closure that draws UI using ratatui
    ///
    /// # Example
    ///
    /// ```ignore
    /// # use bevy::prelude::*;
    /// # use bevy_tui_texture::setup::TerminalTexture;
    /// # use ratatui::widgets::Paragraph;
    /// # fn render(mut texture: TerminalTexture, render_device: Res<RenderDevice>, render_queue: Res<RenderQueue>, mut images: ResMut<Assets<Image>>) {
    /// texture.update(&render_device, &render_queue, &mut images, |frame| {
    ///     frame.render_widget(Paragraph::new("Hello!"), frame.area());
    /// });
    /// # }
    /// ```
    pub fn update<F>(
        &mut self,
        render_device: &RenderDevice,
        render_queue: &RenderQueue,
        images: &mut ResMut<Assets<Image>>,
        draw_fn: F,
    ) where
        F: FnOnce(&mut ratatui::Frame),
    {
        // Step 1: Retrieve previous frame's async copy (non-blocking)
        if let Some(async_copy) = self.pending_copy.take() {
            let _ = render_device.wgpu_device().poll(wgpu::PollType::Poll);

            if async_copy.is_ready() {
                // Copy completed buffer data to Bevy Image
                if let Some(image) = images.get_mut(&self.image_handle) {
                    async_copy.copy_to_image(image);
                }
                async_copy.buffer.unmap();
            } else {
                // Not ready yet, restore it for next frame
                self.pending_copy = Some(async_copy);
            }
        }

        // Step 2: Draw new frame
        let _ = self.terminal.draw(draw_fn);

        // Step 3: Render to GPU texture
        let texture_view = self
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.terminal.backend_mut().render_to_texture(
            render_device.wgpu_device(),
            render_queue.0.as_ref(),
            &texture_view,
        );

        // Step 4: Issue async copy for current frame (non-blocking)
        self.pending_copy = Some(AsyncCopy::from_texture(
            &self.texture,
            self.width,
            self.height,
            render_device,
            render_queue,
        ));
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

/// Simplified terminal for 2D scenes with automatic entity spawning.
///
/// This provides a complete 2D terminal setup in a single call, including:
/// - Texture and terminal creation
/// - Entity spawning with ImageNode and Node components
/// - Optional input handling (keyboard and mouse)
/// - Automatic entity and material management
///
/// Perfect for 2D UI overlays, HUDs, and traditional TUI applications.
pub struct SimpleTerminal2D {
    texture_state: TerminalTexture,
    entity_id: Entity,
}

impl SimpleTerminal2D {
    /// Create and spawn a complete 2D terminal in one call.
    ///
    /// # Arguments
    ///
    /// * `cols` - Number of columns (characters wide)
    /// * `rows` - Number of rows (characters tall)
    /// * `fonts` - Font configuration (shared via Arc)
    /// * `position` - 2D pixel position (left, top)
    /// * `programmatic_glyphs` - If true, pre-populate box drawing, braille, and powerline glyphs
    /// * `enable_keyboard` - If true, enable keyboard input
    /// * `enable_mouse` - If true, enable mouse input
    /// * `commands` - Bevy Commands for spawning entities
    /// * `render_device` - Bevy's RenderDevice resource
    /// * `render_queue` - Bevy's RenderQueue resource
    /// * `images` - Bevy's Image assets
    ///
    /// # Returns
    ///
    /// Returns `Ok(SimpleTerminal2D)` on success, or an error message on failure.
    ///
    /// # Example
    ///
    /// ```ignore
    /// # use bevy::prelude::*;
    /// # use bevy_tui_texture::setup::SimpleTerminal2D;
    /// # fn setup(mut commands: Commands, render_device: Res<RenderDevice>, render_queue: Res<RenderQueue>, mut images: ResMut<Assets<Image>>) {
    /// let fonts = /* load fonts */;
    /// let terminal = SimpleTerminal2D::create_and_spawn(
    ///     80, 25, fonts, (0.0, 0.0), true, true, true,
    ///     &mut commands, &render_device, &render_queue, &mut images,
    /// ).unwrap();
    /// # }
    /// ```
    #[allow(clippy::too_many_arguments)]
    pub fn create_and_spawn(
        cols: u16,
        rows: u16,
        fonts: Arc<Fonts>,
        position: (f32, f32), // (left, top)
        programmatic_glyphs: bool,
        enable_keyboard: bool,
        enable_mouse: bool,
        commands: &mut Commands,
        render_device: &RenderDevice,
        render_queue: &RenderQueue,
        images: &mut ResMut<Assets<Image>>,
    ) -> Result<Self, String> {
        // Create texture state
        let texture_state = TerminalTexture::create(
            cols,
            rows,
            fonts,
            programmatic_glyphs,
            render_device,
            render_queue,
            images,
        )?;

        // Spawn entity
        let mut entity_builder = commands.spawn((
            ImageNode {
                image: texture_state.image_handle(),
                ..default()
            },
            Node {
                width: Val::Px(texture_state.width as f32),
                height: Val::Px(texture_state.height as f32),
                left: Val::Px(position.0),
                top: Val::Px(position.1),
                ..default()
            },
            GlobalTransform::default(),
            TerminalComponent,
            texture_state.dimensions(),
        ));

        // Add input handling if enabled
        if enable_keyboard || enable_mouse {
            entity_builder.insert(TerminalInput::default());
        }

        let entity_id = entity_builder.id();

        Ok(Self {
            texture_state,
            entity_id,
        })
    }

    /// Get the entity ID of the spawned terminal.
    ///
    /// This allows you to add additional components to the terminal entity,
    /// such as marker components for queries.
    ///
    /// # Example
    ///
    /// ```ignore
    /// # use bevy::prelude::*;
    /// # use bevy_tui_texture::setup::SimpleTerminal2D;
    /// # #[derive(Component)]
    /// # struct MyTerminal;
    /// # fn setup(mut commands: Commands, terminal: SimpleTerminal2D) {
    /// // Add a marker component to the terminal
    /// commands.entity(terminal.entity()).insert(MyTerminal);
    /// # }
    /// ```
    pub fn entity(&self) -> Entity {
        self.entity_id
    }

    /// Get the terminal dimensions.
    ///
    /// Delegates to the underlying `TerminalTexture`.
    pub fn dimensions(&self) -> TerminalDimensions {
        self.texture_state.dimensions()
    }

    /// Get the image handle.
    ///
    /// Delegates to the underlying `TerminalTexture`.
    pub fn image_handle(&self) -> Handle<Image> {
        self.texture_state.image_handle()
    }

    /// Draw and render the terminal in one call.
    ///
    /// This method:
    /// 1. Calls the provided drawing function with a ratatui Frame
    /// 2. Renders the terminal to the GPU texture
    /// 3. Copies the GPU texture to the Bevy Image
    ///
    /// The material automatically updates from the image, so no manual
    /// material update is needed for 2D terminals.
    ///
    /// # Arguments
    ///
    /// * `render_device` - Bevy's RenderDevice resource
    /// * `render_queue` - Bevy's RenderQueue resource
    /// * `images` - Bevy's Image assets
    /// * `draw_fn` - Closure that draws UI using ratatui
    ///
    /// # Example
    ///
    /// ```ignore
    /// # use bevy::prelude::*;
    /// # use bevy_tui_texture::setup::SimpleTerminal2D;
    /// # use ratatui::widgets::Paragraph;
    /// # fn render(mut terminal: SimpleTerminal2D, render_device: Res<RenderDevice>, render_queue: Res<RenderQueue>, mut images: ResMut<Assets<Image>>) {
    /// terminal.draw_and_render(&render_device, &render_queue, &mut images, |frame| {
    ///     frame.render_widget(Paragraph::new("Hello 2D!"), frame.area());
    /// });
    /// # }
    /// ```
    pub fn draw_and_render<F>(
        &mut self,
        render_device: &RenderDevice,
        render_queue: &RenderQueue,
        images: &mut ResMut<Assets<Image>>,
        draw_fn: F,
    ) where
        F: FnOnce(&mut ratatui::Frame),
    {
        self.texture_state
            .update(render_device, render_queue, images, draw_fn);
    }
}

/// Simplified terminal for 3D scenes with automatic entity spawning.
///
/// This provides a complete 3D terminal setup in a single call, including:
/// - Texture and terminal creation (with proper RenderAssetUsages for 3D)
/// - Entity spawning with Mesh3d and StandardMaterial components
/// - Full 3D transform support (position, rotation, scale)
/// - Optional input handling (keyboard and mouse with 3D raycasting)
/// - Marker component support for query filtering
///
/// Perfect for in-game computer terminals, futuristic displays, and spatial UIs.
pub struct SimpleTerminal3D {
    texture_state: TerminalTexture,
    entity_id: Entity,
}

impl SimpleTerminal3D {
    /// Create and spawn a complete 3D terminal in one call.
    ///
    /// # Arguments
    ///
    /// * `cols` - Number of columns (characters wide)
    /// * `rows` - Number of rows (characters tall)
    /// * `fonts` - Font configuration (shared via Arc)
    /// * `position` - 3D world position (Vec3)
    /// * `rotation` - 3D rotation (Quat) - use `Quat::from_rotation_x(-FRAC_PI_2)` to face camera
    /// * `scale` - 3D scale (Vec3)
    /// * `marker` - Marker component for query filtering (must implement Component)
    /// * `programmatic_glyphs` - If true, pre-populate box drawing, braille, and powerline glyphs
    /// * `enable_keyboard` - If true, enable keyboard input
    /// * `enable_mouse` - If true, enable mouse input (3D raycasting)
    /// * `commands` - Bevy Commands for spawning entities
    /// * `meshes` - Bevy's Mesh assets
    /// * `materials` - Bevy's StandardMaterial assets
    /// * `render_device` - Bevy's RenderDevice resource
    /// * `render_queue` - Bevy's RenderQueue resource
    /// * `images` - Bevy's Image assets
    ///
    /// # Returns
    ///
    /// Returns `Ok(SimpleTerminal3D)` on success, or an error message on failure.
    ///
    /// # Example
    ///
    /// ```ignore
    /// # use bevy::prelude::*;
    /// # use bevy_tui_texture::setup::SimpleTerminal3D;
    /// # #[derive(Component)]
    /// # struct MainTerminal;
    /// # fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, render_device: Res<RenderDevice>, render_queue: Res<RenderQueue>, mut images: ResMut<Assets<Image>>) {
    /// let fonts = /* load fonts */;
    /// let terminal = SimpleTerminal3D::create_and_spawn(
    ///     80, 25, fonts,
    ///     Vec3::ZERO,
    ///     Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),  // Face camera
    ///     Vec3::ONE,
    ///     MainTerminal,
    ///     true, true, true,
    ///     &mut commands, &mut meshes, &mut materials,
    ///     &render_device, &render_queue, &mut images,
    /// ).unwrap();
    /// # }
    /// ```
    #[allow(clippy::too_many_arguments)]
    pub fn create_and_spawn<T: Component>(
        cols: u16,
        rows: u16,
        fonts: Arc<Fonts>,
        position: Vec3,
        rotation: Quat,
        scale: Vec3,
        marker: T,
        programmatic_glyphs: bool,
        enable_keyboard: bool,
        enable_mouse: bool,
        commands: &mut Commands,
        meshes: &mut ResMut<Assets<Mesh>>,
        materials: &mut ResMut<Assets<StandardMaterial>>,
        render_device: &RenderDevice,
        render_queue: &RenderQueue,
        images: &mut ResMut<Assets<Image>>,
    ) -> Result<Self, String> {
        let char_width_px = fonts.min_width_px();
        let char_height_px = fonts.height_px();
        let width = cols as u32 * char_width_px;
        let height = rows as u32 * char_height_px;

        // Create GPU texture
        let texture = render_device
            .wgpu_device()
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("Terminal 3D Texture"),
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

        // Create Bevy Image (3D requires proper RenderAssetUsages)
        let mut image = Image::new_fill(
            bevy::render::render_resource::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            &[0, 0, 0, 255],
            bevy::render::render_resource::TextureFormat::Rgba8Unorm,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        image.texture_descriptor.usage = bevy::render::render_resource::TextureUsages::COPY_DST
            | bevy::render::render_resource::TextureUsages::TEXTURE_BINDING;
        let image_handle = images.add(image);

        // Create backend
        let mut backend = TerminalBuilder::new(fonts)
            .with_dimensions(cols, rows)
            .build(render_device.wgpu_device(), render_queue.0.as_ref())
            .map_err(|e| format!("Failed to build backend: {:?}", e))?;

        if programmatic_glyphs {
            backend
                .populate_programmatic_glyphs(render_queue.0.as_ref())
                .map_err(|e| format!("Failed to populate glyphs: {:?}", e))?;
        }

        let terminal = ratatui::Terminal::new(backend)
            .map_err(|e| format!("Failed to create terminal: {}", e))?;

        // Create 3D mesh plane
        let mesh = meshes.add(Plane3d::default().mesh().size(width as f32, height as f32));
        let material = materials.add(StandardMaterial {
            base_color_texture: Some(image_handle.clone()),
            unlit: true, // Disable lighting for terminal display
            alpha_mode: AlphaMode::Blend,
            ..default()
        });

        // Spawn 3D entity
        let mut entity_builder = commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform {
                translation: position,
                rotation,
                scale,
            },
            marker,
            TerminalComponent,
            TerminalDimensions {
                cols,
                rows,
                char_width_px,
                char_height_px,
            },
        ));

        if enable_keyboard || enable_mouse {
            entity_builder.insert(TerminalInput::default());
        }

        let entity_id = entity_builder.id();

        let texture_state = TerminalTexture {
            terminal,
            texture,
            image_handle,
            width,
            height,
            cols,
            rows,
            char_width_px,
            char_height_px,
            pending_copy: None,
        };

        Ok(Self {
            texture_state,
            entity_id,
        })
    }

    /// Get the entity ID of the spawned 3D terminal.
    ///
    /// This allows you to add additional components to the terminal entity
    /// or query for the specific entity.
    ///
    /// # Example
    ///
    /// ```ignore
    /// # use bevy::prelude::*;
    /// # use bevy_tui_texture::setup::SimpleTerminal3D;
    /// # #[derive(Component)]
    /// # struct ExtraData(String);
    /// # fn setup(mut commands: Commands, terminal: SimpleTerminal3D) {
    /// // Add additional components to the terminal entity
    /// commands.entity(terminal.entity()).insert(ExtraData("Extra".into()));
    /// # }
    /// ```
    pub fn entity(&self) -> Entity {
        self.entity_id
    }

    /// Get the terminal dimensions.
    ///
    /// Delegates to the underlying `TerminalTexture`.
    pub fn dimensions(&self) -> TerminalDimensions {
        self.texture_state.dimensions()
    }

    /// Get the image handle.
    ///
    /// Delegates to the underlying `TerminalTexture`.
    pub fn image_handle(&self) -> Handle<Image> {
        self.texture_state.image_handle()
    }

    /// Draw and render the terminal with StandardMaterial update.
    ///
    /// This method:
    /// 1. Calls the provided drawing function with a ratatui Frame
    /// 2. Renders the terminal to the GPU texture
    /// 3. Copies the GPU texture to the Bevy Image
    /// 4. Updates the StandardMaterial to trigger change detection
    ///
    /// # Arguments
    ///
    /// * `render_device` - Bevy's RenderDevice resource
    /// * `render_queue` - Bevy's RenderQueue resource
    /// * `images` - Bevy's Image assets
    /// * `materials` - Bevy's StandardMaterial assets
    /// * `marker_query` - Query filtered by the marker component
    /// * `draw_fn` - Closure that draws UI using ratatui
    ///
    /// # Example
    ///
    /// ```ignore
    /// # use bevy::prelude::*;
    /// # use bevy_tui_texture::setup::SimpleTerminal3D;
    /// # use ratatui::widgets::Paragraph;
    /// # #[derive(Component)]
    /// # struct MainTerminal;
    /// # fn render(
    /// #     mut terminal: SimpleTerminal3D,
    /// #     render_device: Res<RenderDevice>,
    /// #     render_queue: Res<RenderQueue>,
    /// #     mut images: ResMut<Assets<Image>>,
    /// #     mut materials: ResMut<Assets<StandardMaterial>>,
    /// #     query: Query<&MeshMaterial3d<StandardMaterial>, With<MainTerminal>>,
    /// # ) {
    /// terminal.draw_and_render(
    ///     &render_device, &render_queue, &mut images,
    ///     &mut materials, &query,
    ///     |frame| {
    ///         frame.render_widget(Paragraph::new("Hello 3D!"), frame.area());
    ///     }
    /// );
    /// # }
    /// ```
    pub fn draw_and_render<F, T: Component>(
        &mut self,
        render_device: &RenderDevice,
        render_queue: &RenderQueue,
        images: &mut ResMut<Assets<Image>>,
        materials: &mut ResMut<Assets<StandardMaterial>>,
        marker_query: &Query<&MeshMaterial3d<StandardMaterial>, With<T>>,
        draw_fn: F,
    ) where
        F: FnOnce(&mut ratatui::Frame),
    {
        // Update texture
        self.texture_state
            .update(render_device, render_queue, images, draw_fn);

        // Trigger StandardMaterial change detection
        for material_handle in marker_query.iter() {
            if let Some(material) = materials.get_mut(&material_handle.0) {
                material.base_color_texture = Some(self.texture_state.image_handle());
            }
        }
    }
}
