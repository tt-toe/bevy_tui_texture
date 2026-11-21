//! A Bevy plugin for rendering terminal-style UIs using ratatui and WGPU,
//! displayable on 2D UI nodes, 3D meshes, or existing meshes (e.g. glTF
//! screens).

#![deny(unsafe_code)]

pub mod backend;
pub mod bevy_plugin;
pub(crate) mod colors;
pub mod fonts;
pub mod input;
pub mod setup;
#[cfg(test)]
mod test_fonts;
pub(crate) mod utils;

pub use ratatui;
pub use wgpu;

pub use backend::bevy_backend::{BevyTerminalBackend, CursorStyle, TerminalBuilder};
pub use backend::{Dimensions, Viewport};
pub use colors::TerminalPalette;

pub use fonts::{Font, Fonts, Shaping, TerminalFontAsset};

pub use bevy_plugin::{TerminalDimensions, TerminalPlugin};

#[cfg(feature = "2d")]
pub use setup::TuiUi;
#[cfg(feature = "3d")]
pub use setup::{AttachMaterial, AttachTerminal};
pub use setup::{HitRegions, TerminalConfig, Tui, TuiFontSource, TuiKind, TuiRequest, TuiSurface};

/// Errors that can occur creating or operating a terminal.
#[derive(Debug)]
pub enum TerminalError {
    /// ratatui's own `Terminal::new()` failed.
    Backend(std::io::Error),

    /// Font data failed to parse (see [`fonts::Fonts::from_asset`]).
    Font(String),
}

impl std::fmt::Display for TerminalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Backend(err) => write!(f, "ratatui terminal initialization failed: {err}"),
            Self::Font(msg) => write!(f, "invalid font data: {msg}"),
        }
    }
}

impl std::error::Error for TerminalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Backend(err) => Some(err),
            Self::Font(_) => None,
        }
    }
}

impl From<std::io::Error> for TerminalError {
    fn from(err: std::io::Error) -> Self {
        Self::Backend(err)
    }
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
        BevyTerminalBackend, CursorStyle, Font, Fonts, Shaping, TerminalBuilder, TerminalFontAsset,
        TerminalPalette,
    };

    // `KeyCode` is deliberately left out: bevy's prelude exports its own,
    // and two glob imports of one name are ambiguous. Import
    // `input::KeyCode` explicitly; an explicit import wins over a glob.
    pub use crate::input::{
        CursorPosition, InputEvent, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent,
        MouseEventKind, TerminalEvent, TerminalFocus, TerminalInput, TerminalInputConfig,
    };

    pub use ratatui;
}
