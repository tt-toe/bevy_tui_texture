//! # bevy_tui_texture
//!
//! A Bevy plugin for rendering terminal-style UIs using ratatui and WGPU,
//! displayable on 2D UI nodes, 3D meshes, or existing meshes (e.g. glTF
//! screens).
//!
//! | bevy | ratatui | wgpu | bevy_tui_texture |
//! |------|---------|------|------------------|
//! | 0.19 | 0.30.2  | 29   | 0.3              |
//!
//! The `wgpu` dependency version must exactly match the one pinned inside
//! this bevy version (raw `wgpu` types cross the public API, e.g.
//! [`Tui::wgpu_texture`](setup::Tui::wgpu_texture)) - see the comment above
//! the `wgpu` dependency in `Cargo.toml`.
//!
//! ## Features
//!
//! - **GPU-Accelerated Rendering** - Render ratatui terminal UIs as GPU textures using WGPU
//! - **Flexible Display Options** - Display terminals on Bevy UI nodes, 3D meshes, or existing meshes
//! - **Full Unicode Support** - Complete support for CJK (Chinese, Japanese, Korean) characters
//! - **Interactive Input** - Built-in keyboard and mouse input handling with focus management
//! - **Programmatic Glyphs** - Automatic rendering of box-drawing, block elements, and Braille patterns
//! - **Real-time Updates** - Efficient real-time terminal content updates with minimal overhead
//!
//! ## Quick Start
//!
//! Mirrors `examples/helloworld.rs` - keep the two in sync.
//!
//! ```no_run
//! use bevy::prelude::*;
//! use bevy::render::renderer::{RenderDevice, RenderQueue};
//! use bevy_tui_texture::prelude::*;
//! use bevy_tui_texture::Font as TerminalFont;
//! use font_kit::family_name::FamilyName;
//! use font_kit::properties::Properties;
//! use font_kit::source::SystemSource;
//! use ratatui::prelude::*;
//! use ratatui::style::Color as RatatuiColor;
//! use ratatui::widgets::*;
//! use std::sync::Arc;
//!
//! #[derive(Component)]
//! struct HelloTerminal;
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
//! fn setup(
//!     mut commands: Commands,
//!     render_device: Res<RenderDevice>,
//!     render_queue: Res<RenderQueue>,
//!     mut images: ResMut<Assets<Image>>,
//!     mut meshes: ResMut<Assets<Mesh>>,
//!     mut materials: ResMut<Assets<StandardMaterial>>,
//! ) {
//!     let fonts = {
//!         let font_data = SystemSource::new()
//!             .select_best_match(&[FamilyName::Monospace], &Properties::new())
//!             .expect("No monospace font found on this system")
//!             .load()
//!             .expect("Failed to load font")
//!             .copy_font_data()
//!             .expect("Failed to copy font data");
//!         let font_data: &'static [u8] = Box::leak(font_data.to_vec().into_boxed_slice());
//!         Arc::new(Fonts::new(
//!             TerminalFont::new(font_data).expect("Failed to parse font"),
//!             16,
//!         ))
//!     };
//!
//!     let mut ctx = TerminalSpawnCtx {
//!         render_device: &render_device,
//!         render_queue: &render_queue,
//!         images: &mut images,
//!         meshes: &mut meshes,
//!         materials: &mut materials,
//!     };
//!     let bundle = TerminalBundle::ui(
//!         80,
//!         25,
//!         fonts,
//!         TerminalConfig { keyboard: false, mouse: false, ..default() },
//!         &mut ctx,
//!     )
//!     .expect("Failed to create terminal");
//!
//!     commands.spawn((bundle, Node::default(), HelloTerminal));
//!     commands.spawn(Camera2d);
//! }
//!
//! fn render_terminal(mut screens: Query<&mut Tui, With<HelloTerminal>>) {
//!     let Ok(mut term) = screens.single_mut() else { return };
//!     term.draw(|frame| {
//!         let text = Paragraph::new("Hello, World!")
//!             .style(Style::default().fg(RatatuiColor::Green).bold())
//!             .alignment(Alignment::Center)
//!             .block(Block::bordered().title("Minimal Example"));
//!         frame.render_widget(text, frame.area());
//!     });
//! }
//! ```
//!
//! ## Examples
//!
//! Run any example with `cargo run --example <name>`:
//!
//! - `helloworld` - Minimal 2D terminal
//! - `widget_catalog_2d` / `widget_catalog_3d` - ratatui widgets, mouse hit-testing, CJK
//! - `world_terminal` - World-unit-sized in-game screen (`TerminalBundle::world_quad`)
//! - `multiple_terminals` - Several terminals + Tab focus cycling
//! - `shader_mesh` - `ExtendedMaterial` CRT shader effects
//! - `retro_crt` - glTF model + shader + overlay UI + camera modes
//! - `tui_component` - Manual entity spawning with `TerminalTexture`
//! - `benchmark` - Rendering throughput
//! - `wasm_demo` / `wasm_serve` - WASM build and local server
//!
//! ## Architecture
//!
//! The library is organized into several key modules:
//!
//! - [`bevy_plugin`] - Bevy plugin, resources, and component definitions
//! - [`backend`] - WGPU-based ratatui backend implementation
//! - [`setup`] - Terminal texture creation and ECS spawn helpers
//! - [`fonts`] - Font loading and rendering with Unicode support
//! - [`input`] - Keyboard and mouse input handling system
//!
//! ### Abstraction Ladder
//!
//! 1. [`setup::TerminalTexture`] + [`setup::Tui::from_texture_state`] - manual
//!    entity spawning, maximum flexibility (see `examples/tui_component.rs`).
//! 2. [`setup::TerminalBundle::ui`] / [`setup::TerminalBundle::world_quad`]
//!    (feature `2d`/`3d`) - thin spawn helpers returning a `Bundle`.
//! 3. [`setup::AttachTerminal`] (feature `3d`) - attach a `Tui` to an
//!    *existing* mesh (e.g. a glTF primitive) instead of spawning one.
//!
//! ## Feature Flags
//!
//! - `2d` (default) - 2D UI terminals ([`setup::TuiUi`], `TerminalBundle::ui`)
//! - `3d` (default) - 3D mesh terminals (`TerminalBundle::world_quad`,
//!   [`setup::AttachTerminal`], mesh raycasting)
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
pub use fonts::{Font, Fonts, TerminalFontAsset};

// Re-export bevy plugin types
pub use bevy_plugin::{TerminalDimensions, TerminalPlugin};

// Re-export the ECS-native terminal API
pub use setup::{HitRegions, TerminalConfig, Tui, TuiSurface};
#[cfg(feature = "2d")]
pub use setup::TuiUi;
#[cfg(feature = "3d")]
pub use setup::{AttachMaterial, AttachTerminal};
#[cfg(all(feature = "2d", feature = "3d"))]
pub use setup::{TerminalBundle, TerminalSpawnCtx};

// Error types

/// Errors that can occur creating or operating a terminal. Replaces the
/// previous `Result<_, String>` used throughout `setup`/`backend` - callers
/// can now match on a specific variant or chain `source()` instead of
/// parsing a message string.
#[derive(thiserror::Error, Debug)]
pub enum TerminalError {
    /// Backend creation failed because the device request failed.
    #[error("device request failed: {0}")]
    DeviceRequestFailed(wgpu::RequestDeviceError),
    /// Backend creation failed because creating the surface failed.
    #[error("surface creation failed: {0}")]
    SurfaceCreationFailed(wgpu::CreateSurfaceError),
    /// Backend creation failed because wgpu didn't provide an
    /// [`Adapter`](wgpu::Adapter)
    #[error("adapter request failed: {0}")]
    AdapterRequestFailed(wgpu::RequestAdapterError),
    /// Backend creation failed because the default surface configuration
    /// couldn't be loaded.
    #[error("failed to get default Surface configuration from wgpu")]
    SurfaceConfigurationRequestFailed,

    /// ratatui's own `Terminal::new()` failed.
    #[error("ratatui terminal initialization failed: {0}")]
    Backend(#[from] std::io::Error),

    /// Font data failed to parse (see [`fonts::Fonts::from_asset`]).
    #[error("invalid font data: {0}")]
    Font(String),
}

pub type Result<T> = ::std::result::Result<T, TerminalError>;

type RandomState = std::hash::RandomState;

// Convenience prelude for common imports
pub mod prelude {
    // Plugin and components
    pub use crate::bevy_plugin::{
        TerminalDimensions, TerminalPlugin, TerminalSystemSet, update_terminal_texture,
    };

    pub use crate::setup::TerminalTexture;

    // ECS-native terminal API
    pub use crate::setup::{HitRegions, TerminalConfig, Tui, TuiSurface};
    #[cfg(feature = "2d")]
    pub use crate::setup::TuiUi;
    #[cfg(feature = "3d")]
    pub use crate::setup::{AttachMaterial, AttachTerminal};
    #[cfg(all(feature = "2d", feature = "3d"))]
    pub use crate::setup::{TerminalBundle, TerminalSpawnCtx};

    // Backend and builders
    pub use crate::{BevyTerminalBackend, Font, Fonts, TerminalBuilder, TerminalFontAsset};

    // Input handling
    pub use crate::input::{
        CursorPosition, KeyModifiers, TerminalEvent, TerminalEventType, TerminalFocus,
        TerminalInput, TerminalInputConfig,
    };

    // Re-export ratatui for convenience
    pub use ratatui;
}
