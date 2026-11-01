//! A Bevy plugin for rendering terminal-style UIs using ratatui and WGPU,
//! displayable on 2D UI nodes, 3D meshes, or existing meshes (e.g. glTF
//! screens).

pub mod backend;
pub mod bevy_plugin;
pub(crate) mod colors;
pub mod fonts;
pub mod input;
pub mod setup;
pub(crate) mod utils;

pub use ratatui;
pub use wgpu;

pub use backend::bevy_backend::{BevyTerminalBackend, TerminalBuilder};
pub use backend::{Dimensions, Viewport};

pub use fonts::{Font, Fonts, Shaping, TerminalFontAsset};

pub use bevy_plugin::{TerminalDimensions, TerminalPlugin};

#[cfg(feature = "2d")]
pub use setup::TuiUi;
#[cfg(feature = "3d")]
pub use setup::{AttachMaterial, AttachTerminal};
pub use setup::{HitRegions, TerminalConfig, Tui, TuiFontSource, TuiKind, TuiRequest, TuiSurface};

/// Errors that can occur creating or operating a terminal.
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

pub mod prelude {
    pub use crate::bevy_plugin::{TerminalDimensions, TerminalPlugin, TerminalSystemSet};

    pub use crate::setup::TerminalTexture;

    #[cfg(feature = "2d")]
    pub use crate::setup::TuiUi;
    #[cfg(feature = "3d")]
    pub use crate::setup::{AttachMaterial, AttachTerminal};
    pub use crate::setup::{
        HitRegions, TerminalConfig, Tui, TuiFontSource, TuiKind, TuiRequest, TuiSurface,
    };

    pub use crate::{
        BevyTerminalBackend, Font, Fonts, Shaping, TerminalBuilder, TerminalFontAsset,
    };

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

    pub use ratatui;
}
