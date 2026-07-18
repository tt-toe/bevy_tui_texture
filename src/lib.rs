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
//! this bevy version - see the comment above the `wgpu` dependency in
//! `Cargo.toml`.
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
//! /// No render resources anywhere in this signature - spawn a
//! /// `TuiRequest` and the plugin materializes it next frame.
//! fn setup(mut commands: Commands) {
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
//!     commands.spawn((
//!         TuiRequest::ui(80, 25, fonts).with_config(TerminalConfig {
//!             keyboard: false,
//!             mouse: false,
//!             ..default()
//!         }),
//!         Node::default(),
//!         HelloTerminal,
//!     ));
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
//! - `world_terminal` - World-unit-sized in-game screen (`TuiRequest::world_quad`
//!   + `TuiFontSource::Asset` font loading)
//! - `multiple_terminals` - Several terminals + Tab focus cycling
//! - `shader_mesh` - `ExtendedMaterial` CRT shader effects
//! - `retro_crt` - glTF model + shader + overlay UI + camera modes
//! - `tui_component` - Manual entity spawning with `TerminalTexture`
//! - `resize` - `Tui::request_resize` following the window size live
//! - `transparent_world_quad` - HUD-style see-through screen
//!   (`transparent_reset_bg` + `AlphaMode::Blend`)
//! - `benchmark` - Rendering throughput
//! - `wasm_demo` - the retro CRT scene running in a browser (WebGL2); see
//!   `docs/README.md` for build/deploy/preview instructions
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
//! 2. [`setup::TuiRequest`] - declarative spawning: spawn the request
//!    component ([`setup::TuiKind::Ui`] / `WorldQuad` / `Headless`), the
//!    plugin materializes the terminal - no render resources in user code.
//! 3. [`setup::AttachTerminal`] (feature `3d`) - attach a `Tui` to an
//!    *existing* mesh (e.g. a glTF primitive) instead of spawning one
//!    (combine with a `Headless` request for the `Tui` itself).
//!
//! ### Runtime Resize
//!
//! [`setup::Tui::request_resize`] changes a terminal's grid size live - no
//! despawn/respawn, no GPU work at the call site (applied on the next
//! frame's flush; `ImageNode`/materials keep pointing at the same
//! `Handle<Image>`, recreated in place). There is deliberately no
//! auto-fit-to-window helper - the recipe (see `examples/resize.rs`):
//!
//! ```ignore
//! fn handle_resize(
//!     mut events: MessageReader<TerminalEvent>,
//!     fonts: Res<MyFontsResource>, // whatever holds the Arc<Fonts> the terminal uses
//!     mut terminals: Query<&mut Tui, With<MyTerminalMarker>>,
//! ) {
//!     let Ok(mut term) = terminals.single_mut() else { return };
//!     for event in events.read() {
//!         if let InputEvent::Resize { pixels } = &event.input {
//!             let cols = (pixels.x / fonts.0.min_width_px()).max(1) as u16;
//!             let rows = (pixels.y / fonts.0.height_px()).max(1) as u16;
//!             term.request_resize(cols, rows);
//!         }
//!     }
//! }
//! ```
//!
//! ### Transparency
//!
//! [`setup::TerminalConfig::transparent_reset_bg`] makes cells with no
//! explicit background (`ratatui::style::Color::Reset`, ratatui's own
//! default) render with alpha 0 instead of an opaque fill - combine with
//! [`setup::TerminalConfig::alpha_mode`]` = AlphaMode::Blend` on a
//! `TuiKind::WorldQuad` for a HUD-style screen the scene shows through (see
//! `examples/transparent_world_quad.rs`). Cells with an explicit background
//! color are unaffected - only `Reset` becomes transparent.
//! [`setup::TerminalConfig::initial_fill`] controls the color shown before
//! any content has been drawn (default opaque black).
//!
//! ## Feature Flags
//!
//! - `2d` (default) - 2D UI terminals ([`setup::TuiUi`], [`setup::TuiKind::Ui`])
//! - `3d` (default) - 3D mesh terminals (`TuiKind::WorldQuad`,
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
pub use setup::{TuiFontSource, HitRegions, TerminalConfig, Tui, TuiKind, TuiRequest, TuiSurface};
#[cfg(feature = "2d")]
pub use setup::TuiUi;
#[cfg(feature = "3d")]
pub use setup::{AttachMaterial, AttachTerminal};

// Error types

/// Errors that can occur creating or operating a terminal. Replaces the
/// previous `Result<_, String>` used throughout `setup`/`backend` - callers
/// can now match on a specific variant or chain `source()` instead of
/// parsing a message string.
#[derive(thiserror::Error, Debug)]
pub enum TerminalError {
    /// ratatui's own `Terminal::new()` failed.
    #[error("ratatui terminal initialization failed: {0}")]
    Backend(#[from] std::io::Error),

    /// Font data failed to parse (see [`fonts::Fonts::from_asset`]).
    #[error("invalid font data: {0}")]
    Font(String),
}

pub type Result<T> = ::std::result::Result<T, TerminalError>;

// Convenience prelude for common imports
pub mod prelude {
    // Plugin and components
    pub use crate::bevy_plugin::{TerminalDimensions, TerminalPlugin, TerminalSystemSet};

    pub use crate::setup::TerminalTexture;

    // ECS-native terminal API
    pub use crate::setup::{
        TuiFontSource, HitRegions, TerminalConfig, Tui, TuiKind, TuiRequest, TuiSurface,
    };
    #[cfg(feature = "2d")]
    pub use crate::setup::TuiUi;
    #[cfg(feature = "3d")]
    pub use crate::setup::{AttachMaterial, AttachTerminal};

    // Backend and builders
    pub use crate::{BevyTerminalBackend, Font, Fonts, TerminalBuilder, TerminalFontAsset};

    // Input handling. `KeyCode` is deliberately NOT re-exported here:
    // `bevy::prelude::*` (glob-imported by every example alongside this
    // prelude) already exports its own `KeyCode` (the physical-key enum),
    // and two glob imports of one name are ambiguous at every use site.
    // Import this crate's `KeyCode` explicitly instead:
    // `use bevy_tui_texture::input::KeyCode;` - an explicit `use` always
    // wins over a glob, so it cleanly shadows bevy's.
    pub use crate::input::{
        CursorPosition, InputEvent, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent,
        MouseEventKind, TerminalEvent, TerminalFocus, TerminalInput, TerminalInputConfig,
    };

    // Re-export ratatui for convenience
    pub use ratatui;
}
