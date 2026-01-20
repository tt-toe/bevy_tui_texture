//! Font loading and management with Unicode support.
//!
//! This module provides font handling for terminal text rendering, including:
//!
//! - **TrueType Font Loading** - Load TTF fonts from embedded or external data
//! - **Font Fallback** - Automatic fallback for missing glyphs across multiple fonts
//! - **Style Support** - Regular, bold, italic, and bold-italic font variants
//! - **Unicode Shaping** - Full Unicode support using `rustybuzz` for complex text layout
//! - **CJK Support** - Proper rendering of Chinese, Japanese, and Korean characters
//!
//! ## Basic Usage
//!
//! ```no_run
//! use bevy_tui_texture::Font;
//! use bevy_tui_texture::Fonts;
//! use std::sync::Arc;
//!
//! // Load font from embedded bytes
//! let font_data = include_bytes!("../assets/fonts/Mplus1Code-Regular.ttf");
//! let font = Font::new(font_data).expect("Failed to load font");
//!
//! // Create font collection with 16px height
//! let fonts = Arc::new(Fonts::new(font, 16));
//! ```
//!
//! ## Font Fallback
//!
//! The `Fonts` collection supports multiple fonts with automatic fallback:
//!
//! ```no_run
//! # use bevy_tui_texture::{Font, Fonts};
//! # let primary_font_data: &[u8] = &[];
//! # let cjk_font_data: &[u8] = &[];
//! let primary = Font::new(primary_font_data).unwrap();
//! let cjk = Font::new(cjk_font_data).unwrap();
//!
//! let mut fonts = Fonts::new(primary, 16);
//! fonts.add_regular_fonts([cjk]);  // Fallback for CJK characters
//! ```

use std::hash::BuildHasher;
use std::hash::Hasher;
use std::hash::RandomState;

use tracing::warn;
use ratatui::buffer::Cell;
use rustybuzz::Face;

/// A TrueType font that can be used for text rendering.
///
/// Fonts are loaded from static byte slices (typically embedded via `include_bytes!`)
/// and are identified by a unique hash for caching purposes.
///
/// # Example
///
/// ```no_run
/// use bevy_tui_texture::Font;
///
/// let font_data = include_bytes!("../assets/fonts/Mplus1Code-Regular.ttf");
/// let font = Font::new(font_data).expect("Failed to load font");
/// ```
#[derive(Clone)]
pub struct Font {
    font: Face<'static>,
    advance: f32,
    id: u64,
}

impl Font {
    pub fn new(data: &'static [u8]) -> Option<Self> {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write(data);

        Face::from_slice(data, 0).map(|font| {
            let advance = font
                .glyph_hor_advance(font.glyph_index('m').unwrap_or_default())
                .unwrap_or_default() as f32;
            Self {
                font,
                advance,
                id: hasher.finish(),
            }
        })
    }
}

impl Font {
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn font(&self) -> &Face<'static> {
        &self.font
    }

    pub(crate) fn char_width(&self, height_px: u32) -> u32 {
        let scale = height_px as f32 / self.font.height() as f32;
        (self.advance * scale) as u32
    }
}

/// A collection of fonts to use for rendering. Supports font fallback.
pub struct Fonts {
    char_width: u32,
    char_height: u32,

    last_resort: Font,

    regular: Vec<Font>,
    bold: Vec<Font>,
    italic: Vec<Font>,
    bold_italic: Vec<Font>,
}

impl Fonts {
    /// Create a new, empty set of fonts. The provided font will be used as a
    /// last-resort fallback if no other fonts can render a particular
    /// character. Rendering will attempt to fake bold/italic styles using this
    /// font where appropriate.
    ///
    /// The provided size_px will be the rendered height in pixels of all fonts
    /// in this collection.
    pub fn new(font: Font, size_px: u32) -> Self {
        Self {
            char_width: font.char_width(size_px),
            char_height: size_px,
            last_resort: font,
            regular: vec![],
            bold: vec![],
            italic: vec![],
            bold_italic: vec![],
        }
    }

    /// The height (in pixels) of all fonts.
    #[inline]
    pub fn height_px(&self) -> u32 {
        self.char_height
    }

    /// Debug: Log font metrics
    #[cfg(debug_assertions)]
    pub fn debug_metrics(&self) {
        eprintln!(
            "FONT METRICS: char_width={}, char_height={}",
            self.char_width, self.char_height
        );
        eprintln!(
            "  last_resort font: 'm' width={}",
            self.last_resort.char_width(self.char_height)
        );
    }

    /// Change the height of all fonts in this collection to the specified
    /// height in pixels.
    pub fn set_size_px(&mut self, height_px: u32) {
        self.char_height = height_px;

        self.char_width = std::iter::once(&self.last_resort)
            .chain(self.regular.iter())
            .chain(self.bold.iter())
            .chain(self.italic.iter())
            .chain(self.bold_italic.iter())
            .map(|font| font.char_width(height_px))
            .min()
            .unwrap_or_default();
    }

    /// Add a collection of fonts for various styles. They will automatically be
    /// added to the appropriate fallback font list based on the font's
    /// bold/italic properties. Note that this will automatically organize fonts
    /// by relative width in order to optimize fallback rendering quality. The
    /// ordering of already provided fonts will remain unchanged.
    pub fn add_fonts(&mut self, fonts: impl IntoIterator<Item = Font>) {
        let bold_italic_len = self.bold_italic.len();
        let italic_len = self.italic.len();
        let bold_len = self.bold.len();
        let regular_len = self.regular.len();

        for font in fonts {
            if !font.font().is_monospaced() {
                warn!("Non monospace font used in add_fonts, this may cause unexpected rendering.");
            }

            self.char_width = self.char_width.min(font.char_width(self.char_height));
            if font.font().is_italic() && font.font().is_bold() {
                self.bold_italic.push(font);
            } else if font.font().is_italic() {
                self.italic.push(font);
            } else if font.font().is_bold() {
                self.bold.push(font);
            } else {
                self.regular.push(font);
            }
        }

        self.bold_italic[bold_italic_len..].sort_by_key(|font| font.char_width(self.char_height));
        self.italic[italic_len..].sort_by_key(|font| font.char_width(self.char_height));
        self.bold[bold_len..].sort_by_key(|font| font.char_width(self.char_height));
        self.regular[regular_len..].sort_by_key(|font| font.char_width(self.char_height));
    }

    /// Add a new collection of fonts for regular styled text. These fonts will
    /// come _after_ previously provided fonts in the fallback order.
    pub fn add_regular_fonts(&mut self, fonts: impl IntoIterator<Item = Font>) {
        self.char_width = self.char_width.min(Self::add_fonts_internal(
            &mut self.regular,
            fonts,
            self.char_height,
        ));
    }

    /// TODO
    /// Add a new collection of fonts for bold styled text. These fonts will
    /// come _after_ previously provided fonts in the fallback order.
    ///
    /// You do not have to provide these for bold text to be supported. If no
    /// bold fonts are supplied, rendering will fallback to the regular fonts
    /// with fake bolding.
    pub fn add_bold_fonts(&mut self, fonts: impl IntoIterator<Item = Font>) {
        self.char_width = self.char_width.min(Self::add_fonts_internal(
            &mut self.bold,
            fonts,
            self.char_height,
        ));
    }

    /// TODO
    /// Add a new collection of fonts for italic styled text. These fonts will
    /// come _after_ previously provided fonts in the fallback order.
    ///
    /// It is recommended, but not required, that you provide italic fonts if
    /// your application intends to make use of italics. If no italic fonts
    /// are supplied, rendering will fallback to the regular fonts with fake
    /// italics.
    pub fn add_italic_fonts(&mut self, fonts: impl IntoIterator<Item = Font>) {
        self.char_width = self.char_width.min(Self::add_fonts_internal(
            &mut self.italic,
            fonts,
            self.char_height,
        ));
    }

    /// TODO
    /// Add a new collection of fonts for bold italic styled text. These fonts
    /// will come _after_ previously provided fonts in the fallback order.
    ///
    /// You do not have to provide these for bold text to be supported. If no
    /// bold fonts are supplied, rendering will fallback to the italic fonts
    /// with fake bolding.
    pub fn add_bold_italic_fonts(&mut self, fonts: impl IntoIterator<Item = Font>) {
        self.char_width = self.char_width.min(Self::add_fonts_internal(
            &mut self.bold_italic,
            fonts,
            self.char_height,
        ));
    }
}

impl Fonts {
    /// The minimum width (in pixels) across all fonts.
    pub fn min_width_px(&self) -> u32 {
        self.char_width
    }

    /// Get the last resort font's ID (for programmatic glyph rendering)
    pub(crate) fn last_resort_id(&self) -> u64 {
        self.last_resort.id()
    }

    pub(crate) fn count(&self) -> usize {
        1 + self.bold.len() + self.italic.len() + self.bold_italic.len() + self.regular.len()
    }

    pub(crate) fn font_for_cell(&self, cell: &Cell) -> (&Font, bool, bool) {
        let is_bold = cell.modifier.contains(ratatui::style::Modifier::BOLD);
        let is_italic = cell.modifier.contains(ratatui::style::Modifier::ITALIC);



        // Build priority-ordered list of fonts to try
        let mut fonts_to_try = Vec::new();

        if is_bold && is_italic {
            // Bold + Italic: try bold_italic first, then fall back with fake styling
            fonts_to_try.extend(self.bold_italic.iter().map(|f| (f, false, false)));
            fonts_to_try.extend(self.bold.iter().map(|f| (f, false, true)));
            fonts_to_try.extend(self.italic.iter().map(|f| (f, true, false)));
            fonts_to_try.extend(self.regular.iter().map(|f| (f, true, true)));
        } else if is_bold {
            // Bold only: try bold, then fake bold on regular
            fonts_to_try.extend(self.bold.iter().map(|f| (f, false, false)));
            fonts_to_try.extend(self.regular.iter().map(|f| (f, true, false)));
            fonts_to_try.extend(self.italic.iter().map(|f| (f, true, false)));
            fonts_to_try.extend(self.bold_italic.iter().map(|f| (f, false, false)));
        } else if is_italic {
            // Italic only: try italic, then fake italic on regular
            fonts_to_try.extend(self.italic.iter().map(|f| (f, false, false)));
            fonts_to_try.extend(self.regular.iter().map(|f| (f, false, true)));
            fonts_to_try.extend(self.bold.iter().map(|f| (f, false, true)));
            fonts_to_try.extend(self.bold_italic.iter().map(|f| (f, false, false)));
        } else {
            // Regular: try regular, then any other
            fonts_to_try.extend(self.regular.iter().map(|f| (f, false, false)));
            fonts_to_try.extend(self.bold.iter().map(|f| (f, false, false)));
            fonts_to_try.extend(self.italic.iter().map(|f| (f, false, false)));
            fonts_to_try.extend(self.bold_italic.iter().map(|f| (f, false, false)));
        }

        // Select font with fake styling as last resort
        self.select_font(
            cell.symbol(),
            fonts_to_try,
            is_bold,   // Use fake bold if no real bold font found
            is_italic, // Use fake italic if no real italic font found
        )
    }

    fn select_font<'fonts>(
        &'fonts self,
        cluster: &str,
        fonts: impl IntoIterator<Item = (&'fonts Font, bool, bool)>,
        last_resort_fake_bold: bool,
        last_resort_fake_italic: bool,
    ) -> (&'fonts Font, bool, bool) {
        let mut max = 0;
        let mut font = None;
        for (candidate, fake_bold, fake_italic) in fonts.into_iter().chain(std::iter::once((
            &self.last_resort,
            last_resort_fake_bold,
            last_resort_fake_italic,
        ))) {
            let (count, last_idx) =
                cluster
                    .chars()
                    .enumerate()
                    .fold((0, 0), |(mut count, _), (idx, ch)| {
                        count += usize::from(candidate.font().glyph_index(ch).is_some());
                        (count, idx)
                    });
            if count > max {
                max = count;
                font = Some((candidate, fake_bold, fake_italic));
            }

            if count == last_idx + 1 {
                break;
            }
        }

        *font.get_or_insert((
            &self.last_resort,
            last_resort_fake_bold,
            last_resort_fake_italic,
        ))
    }

    fn add_fonts_internal(
        target: &mut Vec<Font>,
        fonts: impl IntoIterator<Item = Font>,
        char_height: u32,
    ) -> u32 {
        let len = target.len();
        target.extend(fonts);

        target[len..]
            .iter()
            .map(|font| font.char_width(char_height))
            .min()
            .unwrap_or(u32::MAX)
    }
}
