//! # bevy_tui_texture
//!
//! > A production-ready Bevy plugin for rendering terminal-style UIs using ratatui and WGPU
//!
//! This library provides seamless integration between [Bevy](https://bevyengine.org/) 0.17,
//! [ratatui](https://ratatui.rs/) 0.29, and [wgpu](https://wgpu.rs/) 26.0, allowing you to render
//! terminal-style UIs as GPU textures that can be displayed on 2D sprites, 3D meshes, or UI elements.
//!
//! ## Features
//!
//! - **GPU-Accelerated Rendering** - Render ratatui terminal UIs as GPU textures using WGPU
//! - **Flexible Display Options** - Display terminals on Bevy UI nodes, 2D sprites, or 3D meshes
//! - **Full Unicode Support** - Complete support for CJK (Chinese, Japanese, Korean) characters
//! - **Interactive Input** - Built-in keyboard and mouse input handling with focus management
//! - **Programmatic Glyphs** - Automatic rendering of box-drawing, block elements, and Braille patterns
//! - **Real-time Updates** - Efficient real-time terminal content updates with minimal overhead
//! - **Simple Setup API** - Easy-to-use helpers (`SimpleTerminal2D`, `SimpleTerminal3D`) for quick integration
//!
//! ## Quick Start
//!
//! ### Hello World (2D)
//!
//! ```ignore
//! use std::sync::Arc;
//! use bevy::prelude::*;
//! use bevy::render::renderer::{RenderDevice, RenderQueue};
//! use bevy_tui_texture::prelude::*;
//! use ratatui::prelude::*;
//! use ratatui::widgets::*;
//!
//! fn main() {
//!     App::new()
//!         .add_plugins(DefaultPlugins)
//!         .add_plugins(TerminalPlugin::default())
//!         .add_systems(Startup, setup)
//!         .add_systems(Update, render_terminal.in_set(TerminalSystemSet::Render))
//!         .run();
//! }
//!
//! #[derive(Resource)]
//! struct MyTerminal { terminal: SimpleTerminal2D }
//!
//! fn setup(
//!     mut commands: Commands,
//!     render_device: Res<RenderDevice>,
//!     render_queue: Res<RenderQueue>,
//!     mut images: ResMut<Assets<Image>>,
//! ) {
//!     // Load font
//!     let font_data = include_bytes!("../assets/fonts/Mplus1Code-Regular.ttf");
//!     let font = Font::new(font_data).expect("Failed to load font");
//!     let fonts = Arc::new(Fonts::new(font, 16));
//!
//!     // Create terminal - one simple call!
//!     let terminal = SimpleTerminal2D::create_and_spawn(
//!         80, 25, fonts, (0.0, 0.0), true, true, false,
//!         &mut commands, &render_device, &render_queue, &mut images,
//!     ).expect("Failed to create terminal");
//!
//!     commands.spawn(Camera2d);
//!     commands.insert_resource(MyTerminal { terminal });
//! }
//!
//! fn render_terminal(
//!     mut terminal_res: ResMut<MyTerminal>,
//!     render_device: Res<RenderDevice>,
//!     render_queue: Res<RenderQueue>,
//!     mut images: ResMut<Assets<Image>>,
//! ) {
//!     terminal_res.terminal.draw_and_render(
//!         &render_device, &render_queue, &mut images,
//!         |frame| {
//!             let text = Paragraph::new("Hello, World!")
//!                 .style(Style::default().fg(Color::Green).bold())
//!                 .alignment(Alignment::Center)
//!                 .block(Block::bordered().title("My Terminal"));
//!             frame.render_widget(text, frame.area());
//!         },
//!     );
//! }
//! ```
//!
//! ## Examples
//!
//! The `examples/` directory contains comprehensive demonstrations:
//!
//! - `helloworld.rs` - Minimal example showing basic terminal rendering
//! - `widget_catalog_2d.rs` - Showcase of ratatui widgets in 2D
//! - `widget_catalog_3d.rs` - Full widget catalog rendered in 3D space
//! - `terminal_texture_2d.rs` - Display terminal UI on a 2D sprite
//! - `terminal_texture_3d.rs` - Render terminal on a rotating 3D cube
//! - `multiple_terminals.rs` - Managing multiple independent terminals
//! - `shader.rs` - Custom shader effects with terminal textures
//! - `benchmark.rs` - Performance benchmarking
//!
//! Run any example with:
//!
//! ```bash
//! cargo run --example helloworld
//! ```
//!
//! ## Architecture
//!
//! The library is organized into several key modules:
//!
//! - [`bevy_plugin`] - Bevy plugin, resources, and component definitions
//! - [`backend`] - WGPU-based ratatui backend implementation
//! - [`setup`] - Simplified setup utilities ([`SimpleTerminal2D`], [`SimpleTerminal3D`])
//! - [`fonts`] - Font loading and rendering with Unicode support
//! - [`input`] - Keyboard and mouse input handling system
//!
//! ### Three Levels of Abstraction
//!
//! 1. **[`setup::TerminalTexture`]** - Core texture operations only (maximum flexibility)
//! 2. **[`setup::SimpleTerminal2D`]** - Full 2D setup with automatic entity spawning
//! 3. **[`setup::SimpleTerminal3D`]** - Full 3D setup with mesh and material management
//!
//! ## Feature Flags
//!
//! - `keyboard_input` (default) - Enable keyboard event handling
//! - `mouse_input` (default) - Enable mouse event handling for both 2D UI and 3D mesh terminals
//!
//! ## Performance
//!
//! This library is designed for real-time rendering with:
//!
//! - Efficient GPU texture updates
//! - Cached glyph rendering with text atlas
//! - Minimal CPU-GPU data transfer
//! - Smart dirty tracking for terminal cells
//!
//! See `examples/benchmark.rs` for performance metrics.

// Public modules
pub mod backend;
pub mod bevy_plugin;
pub(crate) mod colors;
pub mod fonts;
pub mod input;
pub mod setup;
pub(crate) mod utils;

// Re-export external crates
pub use ratatui;
pub use wgpu;

// Re-export commonly used types from backend
pub use backend::bevy_backend::{BevyTerminalBackend, TerminalBuilder};
pub use backend::{Dimensions, Viewport};

// Re-export font types
pub use fonts::{Font, Fonts};

// Re-export bevy plugin types
pub use bevy_plugin::{TerminalComponent, TerminalDimensions, TerminalPlugin, TerminalResource};

// Error types
use thiserror::Error;

/// Represents the various errors that can occur during operation.
#[derive(Debug, Error)]
pub enum Error {
    /// Backend creation failed because the device request failed.
    #[error("{0}")]
    DeviceRequestFailed(wgpu::RequestDeviceError),
    /// Backend creation failed because creating the surface failed.
    #[error("{0}")]
    SurfaceCreationFailed(wgpu::CreateSurfaceError),
    /// Backend creation failed because wgpu didn't provide an
    /// [`Adapter`](wgpu::Adapter)
    #[error("{0}")]
    AdapterRequestFailed(wgpu::RequestAdapterError),
    /// Backend creation failed because the default surface configuration
    /// couldn't be loaded.
    #[error("Failed to get default Surface configuration from wgpu.")]
    SurfaceConfigurationRequestFailed,
}

pub type Result<T> = ::std::result::Result<T, Error>;

type RandomState = std::hash::RandomState;

// Convenience prelude for common imports
pub mod prelude {
    // Plugin and components
    pub use crate::bevy_plugin::{
        TerminalComponent, TerminalDimensions, TerminalPlugin, TerminalResource, TerminalSystemSet,
        spawn_display_terminal, spawn_interactive_terminal, spawn_positioned_terminal,
        update_material_texture, update_terminal_and_material, update_terminal_texture,
    };

    // Simplified terminal API
    pub use crate::setup::{SimpleTerminal2D, SimpleTerminal3D, TerminalTexture};

    // Backend and builders
    pub use crate::{BevyTerminalBackend, Font, Fonts, TerminalBuilder};

    // Input handling
    pub use crate::input::{
        CursorPosition, KeyModifiers, TerminalEvent, TerminalEventType, TerminalFocus,
        TerminalInput, TerminalInputConfig,
    };

    // Re-export ratatui for convenience
    pub use ratatui;
}
