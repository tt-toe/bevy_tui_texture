//! Font loading and management with Unicode support.

use std::hash::BuildHasher;
use std::hash::Hasher;
use std::hash::RandomState;
use std::sync::Arc;
use std::sync::Mutex;

use bevy::asset::io::Reader;
use bevy::asset::{Asset, AssetLoader, LoadContext};
use bevy::reflect::TypePath;
use ratatui::buffer::Cell;
use rustybuzz::Face;
use tracing::warn;

self_cell::self_cell!(
    /// Runtime-loaded font bytes and the `Face` parsed from them.
    struct OwnedFace {
        owner: std::sync::Arc<[u8]>,
        #[covariant]
        dependent: Face,
    }
);

#[derive(Clone)]
enum FontFace {
    Static(Box<Face<'static>>),
    Owned(std::sync::Arc<OwnedFace>),
}

/// A TrueType font that can be used for text rendering.
#[derive(Clone)]
pub struct Font {
    face: FontFace,
    advance: f32,
    id: u64,
}

impl Font {
    /// Load a font from a static byte slice (e.g. `include_bytes!`).
    pub fn new(data: &'static [u8]) -> Option<Self> {
        let face = Face::from_slice(data, 0)?;
        Some(Self::build(
            data,
            &face,
            FontFace::Static(Box::new(face.clone())),
        ))
    }

    /// Load a font from owned bytes (e.g. `std::fs::read` at runtime).
    pub fn from_vec(data: Vec<u8>) -> Option<Self> {
        let owned =
            OwnedFace::try_new(data.into(), |bytes| Face::from_slice(bytes, 0).ok_or(())).ok()?;
        let owned = std::sync::Arc::new(owned);
        Some(Self::build(
            owned.borrow_owner(),
            owned.borrow_dependent(),
            FontFace::Owned(std::sync::Arc::clone(&owned)),
        ))
    }

    fn build(data: &[u8], face: &Face<'_>, store: FontFace) -> Self {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write(data);
        let advance = face
            .glyph_hor_advance(face.glyph_index('m').unwrap_or_default())
            .unwrap_or_default() as f32;
        Self {
            face: store,
            advance,
            id: hasher.finish(),
        }
    }
}

impl Font {
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn font(&self) -> &Face<'_> {
        match &self.face {
            FontFace::Static(face) => face,
            FontFace::Owned(owned) => owned.borrow_dependent(),
        }
    }

    pub(crate) fn char_width(&self, height_px: u32) -> u32 {
        let scale = height_px as f32 / self.font().height() as f32;
        (self.advance * scale) as u32
    }
}

/// Font bytes loadable via the `AssetServer` - `Font::from_vec` +
/// `std::fs::read` doesn't work on Wasm, so this is the Wasm-safe path.
#[derive(Asset, TypePath)]
pub struct TerminalFontAsset {
    bytes: Vec<u8>,
}

/// Loads `.ttf` files as [`TerminalFontAsset`].
#[derive(Default, TypePath)]
pub(crate) struct TerminalFontAssetLoader;

impl AssetLoader for TerminalFontAssetLoader {
    type Asset = TerminalFontAsset;
    type Settings = ();
    type Error = std::io::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(TerminalFontAsset { bytes })
    }

    fn extensions(&self) -> &[&str] {
        &["ttf"]
    }
}

/// Shared CPU-side glyph cache state: the `Atlas` slot allocator, the
/// rustybuzz `PlanCache`, and queued-but-not-yet-uploaded glyph
/// rasterizations.
pub(crate) struct SharedFontCpuState {
    pub(crate) cached: crate::utils::text_atlas::Atlas,
    pub(crate) plan_cache: crate::utils::plan_cache::PlanCache,
    pub(crate) pending_cache_updates: Vec<(crate::utils::text_atlas::CacheRect, Vec<u8>)>,
    /// Memoized per-cell font fallback - see [`Fonts::font_for_cell_cached`].
    pub(crate) font_choices: crate::utils::shape_cache::FontChoiceCache,
    /// Shaped single cells - see [`crate::utils::shape_cache::CellShapeCache`].
    pub(crate) cell_shapes: crate::utils::shape_cache::CellShapeCache,
    /// Slot index of the color glyph atlas (premultiplied RGBA, its own GPU
    /// texture, created on the first color glyph) - see
    /// [`crate::backend::bevy_backend`]'s `draw_color_glyph`.
    #[cfg(feature = "emoji")]
    pub(crate) color: crate::utils::text_atlas::Atlas,
    /// The color atlas texture's size in pixels.
    #[cfg(feature = "emoji")]
    pub(crate) color_size: (u32, u32),
    /// Queued color glyph rasterizations (premultiplied RGBA8), one-shot
    /// like `pending_cache_updates`.
    #[cfg(feature = "emoji")]
    pub(crate) pending_color_updates: Vec<(crate::utils::text_atlas::CacheRect, Vec<u8>)>,
}

impl SharedFontCpuState {
    /// Changes whenever either atlas reassigns a slot - a cached row's
    /// instances are only valid while this is unchanged.
    pub(crate) fn atlas_generation(&self) -> u64 {
        #[cfg(feature = "emoji")]
        return self.cached.generation() + self.color.generation();
        #[cfg(not(feature = "emoji"))]
        self.cached.generation()
    }

    fn new(fonts: &Fonts) -> Self {
        use crate::backend::{CACHE_HEIGHT, CACHE_WIDTH};
        // Room for 1024 slots (a full 80x25 screen of distinct emoji), capped
        // at the WebGL2 texture limit. Only the GPU texture is lazy: an empty
        // `Atlas` holds almost nothing.
        #[cfg(feature = "emoji")]
        let (color_size, color) = {
            let (slot_w, slot_h) = (fonts.min_width_px() * 2, fonts.height_px());
            let size = ((32 * slot_w).min(2048), (32 * slot_h).min(2048));
            (
                size,
                crate::utils::text_atlas::Atlas::new(fonts, size.0, size.1),
            )
        };
        Self {
            cached: crate::utils::text_atlas::Atlas::new(fonts, CACHE_WIDTH, CACHE_HEIGHT),
            plan_cache: crate::utils::plan_cache::PlanCache::new(fonts.count().max(2) * 4),
            pending_cache_updates: Vec::new(),
            font_choices: crate::utils::shape_cache::FontChoiceCache::new(),
            cell_shapes: Default::default(),
            #[cfg(feature = "emoji")]
            color,
            #[cfg(feature = "emoji")]
            color_size,
            #[cfg(feature = "emoji")]
            pending_color_updates: Vec::new(),
        }
    }
}

/// What is handed to the text shaper as one unit - see
/// [`Fonts::set_shaping`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shaping {
    /// Each cell is shaped on its own and cached by its symbol, so shaping
    /// runs once per distinct symbol and a row costs the same whether or
    /// not its text was seen before.
    #[default]
    Cell,
    /// Each stretch of a row that uses one font is shaped as a single run,
    /// so the font's contextual substitutions apply across cells.
    Row,
}

/// Underline and strikeout bars in pixels from the cell top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DecorationMetrics {
    pub(crate) underline_top: u32,
    pub(crate) underline_thickness: u32,
    pub(crate) strikeout_top: u32,
    pub(crate) strikeout_thickness: u32,
}

/// A collection of fonts to use for rendering.
pub struct Fonts {
    char_width: u32,
    char_height: u32,
    shaping: Shaping,
    fit_symbols: bool,

    last_resort: Font,

    regular: Vec<Font>,
    bold: Vec<Font>,
    italic: Vec<Font>,
    bold_italic: Vec<Font>,

    /// Color emoji fallback - see [`Fonts::add_emoji_fonts`].
    #[cfg(feature = "emoji")]
    emoji: Vec<Font>,

    /// See [`SharedFontCpuState`].
    shared_cpu_state: Mutex<Option<SharedFontCpuState>>,
}

impl Fonts {
    /// Create a new, empty set of fonts.
    pub fn new(font: Font, size_px: u32) -> Self {
        Self {
            char_width: font.char_width(size_px),
            char_height: size_px,
            shaping: Shaping::default(),
            fit_symbols: true,
            last_resort: font,
            regular: vec![],
            bold: vec![],
            italic: vec![],
            bold_italic: vec![],
            #[cfg(feature = "emoji")]
            emoji: vec![],
            shared_cpu_state: Mutex::new(None),
        }
    }

    /// Chooses how text is shaped (default [`Shaping::Cell`]).
    pub fn set_shaping(&mut self, shaping: Shaping) {
        self.shaping = shaping;
    }

    /// The shaping mode set by [`Self::set_shaping`].
    pub fn shaping(&self) -> Shaping {
        self.shaping
    }

    /// Whether symbols are fitted to the cell grid (default `true`).
    pub fn set_fit_symbols(&mut self, enabled: bool) {
        self.fit_symbols = enabled;
    }

    /// Whether symbols are fitted to the cell grid - see
    /// [`Self::set_fit_symbols`].
    pub fn fit_symbols(&self) -> bool {
        self.fit_symbols
    }

    /// The cell grid symbol constraints resolve against.
    pub(crate) fn cell_metrics(&self) -> crate::backend::glyph_constraint::CellMetrics {
        let primary = self.regular.first().unwrap_or(&self.last_resort).font();
        let cap_height = primary
            .capital_height()
            .filter(|&cap| cap > 0)
            .or_else(|| {
                let h = primary.glyph_index('H')?;
                Some(primary.glyph_bounding_box(h)?.y_max)
            })
            .map_or(primary.ascender() as f32 * 0.75, f32::from);
        let cell_height = self.char_height as f32;
        let cap_px = cap_height * cell_height / primary.height() as f32;
        crate::backend::glyph_constraint::CellMetrics {
            cell_width: self.char_width as f32,
            cell_height,
            icon_height_single: (2.0 * cap_px + cell_height) / 3.0,
        }
    }

    /// Where the primary font puts underline and strikeout bars in this cell grid.
    pub(crate) fn decoration_metrics(&self) -> DecorationMetrics {
        let face = self.regular.first().unwrap_or(&self.last_resort).font();
        let cell_height = self.char_height;
        let scale = cell_height as f32 / face.height() as f32;
        let baseline = face.ascender() as f32 * scale;
        let px = |units: f32| (units * scale).round().max(1.0) as u32;
        let bar = |metrics: Option<rustybuzz::ttf_parser::LineMetrics>| {
            metrics.filter(|m| m.thickness > 0).map(|m| {
                (
                    px(m.thickness as f32),
                    (baseline - m.position as f32 * scale).round(),
                )
            })
        };

        let fallback_thickness = ((cell_height as f32 / 16.0).round() as u32).max(1);
        let (underline_thickness, underline_top) = bar(face.underline_metrics()).unwrap_or((
            fallback_thickness,
            baseline.round() + fallback_thickness as f32,
        ));
        let (strikeout_thickness, strikeout_top) =
            bar(face.strikeout_metrics()).unwrap_or_else(|| {
                let x_height = face
                    .x_height()
                    .map_or(face.ascender() as f32 * 0.5, f32::from);
                (
                    underline_thickness,
                    (baseline - x_height * scale / 2.0).round() - (underline_thickness / 2) as f32,
                )
            });

        let clamp = |thickness: u32, top: f32| {
            let thickness = thickness.min(cell_height).max(1);
            (
                thickness,
                (top.max(0.0) as u32).min(cell_height.saturating_sub(thickness)),
            )
        };
        let (underline_thickness, underline_top) = clamp(underline_thickness, underline_top);
        let (strikeout_thickness, strikeout_top) = clamp(strikeout_thickness, strikeout_top);
        DecorationMetrics {
            underline_top,
            underline_thickness,
            strikeout_top,
            strikeout_thickness,
        }
    }

    /// Whether `ch`, drawn by `font`, is a symbol - see
    /// [`crate::backend::glyph_constraint::CellClass::of`].
    fn cell_class(&self, ch: char, font: &Font) -> crate::backend::glyph_constraint::CellClass {
        use crate::backend::glyph_constraint::CellClass;
        if crate::backend::programmatic_glyphs::fallback_symbols::is_fallback_symbol(ch)
            && font.font().glyph_index(ch).is_none()
        {
            return CellClass {
                fallback: true,
                ..CellClass::TEXT
            };
        }
        if !self.fit_symbols {
            return CellClass::TEXT;
        }
        let advance_px = || {
            let face = font.font();
            let advance = face
                .glyph_index(ch)
                .and_then(|glyph| face.glyph_hor_advance(glyph))
                .unwrap_or_default();
            advance as f32 * self.char_height as f32 / face.height() as f32
        };
        CellClass::of(ch, advance_px, self.char_width as f32)
    }

    /// Stable identity for this `Fonts` instance - used to key the
    /// render-world's per-font shared GPU atlas store.
    pub(crate) fn identity(&self) -> usize {
        self as *const Self as usize
    }

    /// Runs `f` against this `Fonts`' shared CPU-side glyph cache state,
    /// creating it on first use.
    pub(crate) fn with_shared_cpu_state<R>(
        &self,
        f: impl FnOnce(&mut SharedFontCpuState) -> R,
    ) -> R {
        let mut guard = self
            .shared_cpu_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let state = guard.get_or_insert_with(|| SharedFontCpuState::new(self));
        f(state)
    }

    /// Build from a loaded [`TerminalFontAsset`].
    pub fn from_asset(
        asset: &TerminalFontAsset,
        size_px: u32,
    ) -> Result<Arc<Fonts>, crate::TerminalError> {
        let font = Font::from_vec(asset.bytes.clone())
            .ok_or_else(|| crate::TerminalError::Font("failed to parse font data".to_string()))?;
        Ok(Arc::new(Fonts::new(font, size_px)))
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

    /// Add a collection of fonts for various styles.
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

        let char_height = self.char_height;
        for (fonts, from) in [
            (&mut self.bold_italic, bold_italic_len),
            (&mut self.italic, italic_len),
            (&mut self.bold, bold_len),
            (&mut self.regular, regular_len),
        ] {
            if let Some(added) = fonts.get_mut(from..) {
                added.sort_by_key(|font| font.char_width(char_height));
            }
        }
    }

    /// Add a new collection of fonts for regular styled text.
    pub fn add_regular_fonts(&mut self, fonts: impl IntoIterator<Item = Font>) {
        self.char_width = self.char_width.min(Self::add_fonts_internal(
            &mut self.regular,
            fonts,
            self.char_height,
        ));
    }

    /// TODO
    /// Add a new collection of fonts for bold styled text.
    pub fn add_bold_fonts(&mut self, fonts: impl IntoIterator<Item = Font>) {
        self.char_width = self.char_width.min(Self::add_fonts_internal(
            &mut self.bold,
            fonts,
            self.char_height,
        ));
    }

    /// TODO
    /// Add a new collection of fonts for italic styled text.
    pub fn add_italic_fonts(&mut self, fonts: impl IntoIterator<Item = Font>) {
        self.char_width = self.char_width.min(Self::add_fonts_internal(
            &mut self.italic,
            fonts,
            self.char_height,
        ));
    }

    /// TODO
    /// Add a new collection of fonts for bold italic styled text.
    pub fn add_bold_italic_fonts(&mut self, fonts: impl IntoIterator<Item = Font>) {
        self.char_width = self.char_width.min(Self::add_fonts_internal(
            &mut self.bold_italic,
            fonts,
            self.char_height,
        ));
    }
}

#[cfg(feature = "emoji")]
impl Fonts {
    /// Fallback fonts for emoji, in color (COLRv0, COLRv1, CBDT or sbix;
    /// not OT-SVG) or monochrome.
    pub fn add_emoji_fonts(&mut self, fonts: impl IntoIterator<Item = Font>) {
        self.emoji.extend(fonts);
    }

    pub(crate) fn is_emoji_font(&self, id: u64) -> bool {
        self.emoji.iter().any(|font| font.id() == id)
    }

    fn emoji_font_with(&self, ch: char) -> Option<&Font> {
        self.emoji
            .iter()
            .find(|font| font.font().glyph_index(ch).is_some())
    }

    /// A text font with `ch`: the style slots and the last resort, never an
    /// emoji font.
    pub(crate) fn text_font_with(&self, ch: char) -> Option<&Font> {
        [&self.regular, &self.bold, &self.italic, &self.bold_italic]
            .into_iter()
            .flatten()
            .chain(std::iter::once(&self.last_resort))
            .find(|font| font.font().glyph_index(ch).is_some())
    }
}

/// Whether a cell's symbol is shown as an emoji: it starts with a default
/// emoji-presentation char (and not `U+FE0E`), or carries a selector,
/// joiner, keycap or skin tone, or is a flag.
#[cfg(feature = "emoji")]
pub(crate) fn emoji_presentation(symbol: &str) -> bool {
    use unicode_properties::{EmojiStatus, UnicodeEmoji};
    const REGIONAL: std::ops::RangeInclusive<char> = '\u{1F1E6}'..='\u{1F1FF}';
    let mut chars = symbol.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if symbol.contains('\u{FE0E}') {
        return false;
    }
    let default_emoji = !REGIONAL.contains(&first)
        && matches!(
            first.emoji_status(),
            EmojiStatus::EmojiPresentation
                | EmojiStatus::EmojiPresentationAndModifierBase
                | EmojiStatus::EmojiPresentationAndEmojiComponent
                | EmojiStatus::EmojiPresentationAndModifierAndEmojiComponent
        );
    if chars.next().is_none() {
        return default_emoji;
    }
    default_emoji
        || REGIONAL.contains(&first)
        || symbol.chars().any(|c| {
            matches!(
                c,
                '\u{FE0F}' | '\u{200D}' | '\u{20E3}' | '\u{1F3FB}'..='\u{1F3FF}'
            )
        })
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
        #[cfg(feature = "emoji")]
        let emoji = self.emoji.len();
        #[cfg(not(feature = "emoji"))]
        let emoji = 0;
        1 + self.bold.len()
            + self.italic.len()
            + self.bold_italic.len()
            + self.regular.len()
            + emoji
    }

    /// [`Self::font_for_cell`] plus the cell's symbol class (see
    /// [`Self::set_fit_symbols`]), memoized through `cache` for the usual
    /// single-`char` symbol.
    pub(crate) fn font_for_cell_cached(
        &self,
        cell: &Cell,
        cache: &mut crate::utils::shape_cache::FontChoiceCache,
    ) -> (
        &Font,
        bool,
        bool,
        crate::backend::glyph_constraint::CellClass,
    ) {
        let mut chars = cell.symbol().chars();
        let (Some(ch), None) = (chars.next(), chars.next()) else {
            let (font, fake_bold, fake_italic) = self.font_for_cell(cell);
            let first = cell.symbol().chars().next().unwrap_or(' ');
            return (font, fake_bold, fake_italic, self.cell_class(first, font));
        };
        let style = u8::from(cell.modifier.contains(ratatui::style::Modifier::BOLD))
            | u8::from(cell.modifier.contains(ratatui::style::Modifier::ITALIC)) << 1;
        let (id, fake_bold, fake_italic, class) = cache.get_or_insert_with(ch, style, || {
            let (font, fake_bold, fake_italic) = self.font_for_cell(cell);
            (font.id(), fake_bold, fake_italic, self.cell_class(ch, font))
        });
        (self.font_by_id(id), fake_bold, fake_italic, class)
    }

    /// The font with this id.
    fn font_by_id(&self, id: u64) -> &Font {
        if self.last_resort.id() == id {
            return &self.last_resort;
        }
        let found = [&self.regular, &self.bold, &self.italic, &self.bold_italic]
            .into_iter()
            .flatten()
            .find(|font| font.id() == id);
        // Without this an emoji font's id would silently resolve to the
        // last resort.
        #[cfg(feature = "emoji")]
        let found = found.or_else(|| self.emoji.iter().find(|font| font.id() == id));
        found.unwrap_or(&self.last_resort)
    }

    pub(crate) fn font_for_cell(&self, cell: &Cell) -> (&Font, bool, bool) {
        #[cfg(feature = "emoji")]
        let first = cell.symbol().chars().next().unwrap_or(' ');
        #[cfg(feature = "emoji")]
        if emoji_presentation(cell.symbol())
            && let Some(font) = self.emoji_font_with(first)
        {
            return (font, false, false);
        }

        let is_bold = cell.modifier.contains(ratatui::style::Modifier::BOLD);
        let is_italic = cell.modifier.contains(ratatui::style::Modifier::ITALIC);

        // Priority-ordered font slots, each with the fake styling it
        // implies. A fixed array rather than a collected `Vec`: this runs
        // for every cell of every reshaped row (`font_runs` in
        // `bevy_backend.rs`), so it must not allocate.
        let slots: [(&[Font], bool, bool); 4] = if is_bold && is_italic {
            [
                (&self.bold_italic[..], false, false),
                (&self.bold[..], false, true),
                (&self.italic[..], true, false),
                (&self.regular[..], true, true),
            ]
        } else if is_bold {
            [
                (&self.bold[..], false, false),
                (&self.regular[..], true, false),
                (&self.italic[..], true, false),
                (&self.bold_italic[..], false, false),
            ]
        } else if is_italic {
            [
                (&self.italic[..], false, false),
                (&self.regular[..], false, true),
                (&self.bold[..], false, true),
                (&self.bold_italic[..], false, false),
            ]
        } else {
            [
                (&self.regular[..], false, false),
                (&self.bold[..], false, false),
                (&self.italic[..], false, false),
                (&self.bold_italic[..], false, false),
            ]
        };
        let fonts_to_try = slots
            .into_iter()
            .flat_map(|(fonts, fake_bold, fake_italic)| {
                fonts.iter().map(move |font| (font, fake_bold, fake_italic))
            });

        let chosen = self.select_font(cell.symbol(), fonts_to_try, is_bold, is_italic);
        #[cfg(feature = "emoji")]
        if chosen.0.font().glyph_index(first).is_none()
            && let Some(font) = self.emoji_font_with(first)
        {
            return (font, false, false);
        }
        chosen
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

        target
            .iter()
            .skip(len)
            .map(|font| font.char_width(char_height))
            .min()
            .unwrap_or(u32::MAX)
    }
}

#[cfg(test)]
mod font_for_cell_tests {
    use super::*;
    use ratatui::buffer::Cell;
    use ratatui::style::Modifier;

    const FONT_DATA: &[u8] = include_bytes!("../examples/assets/fonts/Mplus1Code-Regular.ttf");

    fn fresh_font() -> Font {
        Font::from_vec(FONT_DATA.to_vec()).expect("failed to load test font")
    }

    fn cell_with(modifier: Modifier) -> Cell {
        let mut cell = Cell::default();
        cell.set_symbol("a");
        cell.modifier = modifier;
        cell
    }

    #[test]
    fn regular_cell_prefers_the_regular_slot() {
        let mut fonts = Fonts::new(fresh_font(), 16);
        let regular = fresh_font();
        let regular_id = regular.id();
        fonts.add_regular_fonts([regular]);

        let (font, fake_bold, fake_italic) = fonts.font_for_cell(&cell_with(Modifier::empty()));
        assert_eq!(font.id(), regular_id);
        assert!(!fake_bold && !fake_italic);
    }

    #[test]
    fn bold_cell_prefers_the_bold_slot_over_regular() {
        let mut fonts = Fonts::new(fresh_font(), 16);
        let bold = fresh_font();
        let bold_id = bold.id();
        fonts.add_regular_fonts([fresh_font()]);
        fonts.add_bold_fonts([bold]);

        let (font, fake_bold, fake_italic) = fonts.font_for_cell(&cell_with(Modifier::BOLD));
        assert_eq!(
            font.id(),
            bold_id,
            "a real bold font must win over faking bold on the regular font"
        );
        assert!(
            !fake_bold && !fake_italic,
            "a real bold font needs no faking"
        );
    }

    #[test]
    fn bold_cell_falls_back_to_faking_bold_on_regular_when_no_bold_slot() {
        let mut fonts = Fonts::new(fresh_font(), 16);
        let regular = fresh_font();
        let regular_id = regular.id();
        fonts.add_regular_fonts([regular]);

        let (font, fake_bold, fake_italic) = fonts.font_for_cell(&cell_with(Modifier::BOLD));
        assert_eq!(
            font.id(),
            regular_id,
            "with no bold font, the regular font must be reused"
        );
        assert!(fake_bold, "bold must be faked on top of the regular font");
        assert!(!fake_italic);
    }

    #[test]
    fn bold_cell_falls_back_to_last_resort_when_nothing_else_registered() {
        let last_resort = fresh_font();
        let last_resort_id = last_resort.id();
        let fonts = Fonts::new(last_resort, 16);

        let (font, fake_bold, _) = fonts.font_for_cell(&cell_with(Modifier::BOLD));
        assert_eq!(font.id(), last_resort_id);
        assert!(fake_bold, "bold must be faked on the last-resort font too");
    }

    #[test]
    fn italic_cell_prefers_the_italic_slot_over_regular() {
        let mut fonts = Fonts::new(fresh_font(), 16);
        let italic = fresh_font();
        let italic_id = italic.id();
        fonts.add_regular_fonts([fresh_font()]);
        fonts.add_italic_fonts([italic]);

        let (font, fake_bold, fake_italic) = fonts.font_for_cell(&cell_with(Modifier::ITALIC));
        assert_eq!(font.id(), italic_id);
        assert!(!fake_bold && !fake_italic);
    }

    #[test]
    fn bold_italic_cell_prefers_the_bold_italic_slot_over_everything() {
        let mut fonts = Fonts::new(fresh_font(), 16);
        let bold_italic = fresh_font();
        let bold_italic_id = bold_italic.id();
        fonts.add_regular_fonts([fresh_font()]);
        fonts.add_bold_fonts([fresh_font()]);
        fonts.add_italic_fonts([fresh_font()]);
        fonts.add_bold_italic_fonts([bold_italic]);

        let (font, fake_bold, fake_italic) =
            fonts.font_for_cell(&cell_with(Modifier::BOLD | Modifier::ITALIC));
        assert_eq!(font.id(), bold_italic_id);
        assert!(!fake_bold && !fake_italic);
    }

    #[test]
    fn bold_italic_cell_falls_back_to_bold_slot_with_faked_italic() {
        let mut fonts = Fonts::new(fresh_font(), 16);
        let bold = fresh_font();
        let bold_id = bold.id();
        fonts.add_regular_fonts([fresh_font()]);
        fonts.add_bold_fonts([bold]);

        let (font, fake_bold, fake_italic) =
            fonts.font_for_cell(&cell_with(Modifier::BOLD | Modifier::ITALIC));
        assert_eq!(
            font.id(),
            bold_id,
            "with no bold_italic slot, the bold font is the next-best fallback"
        );
        assert!(
            !fake_bold && fake_italic,
            "bold is real (no faking needed) but italic must be faked on top"
        );
    }
}

#[cfg(all(test, feature = "emoji"))]
mod emoji_tests {
    use super::*;

    const MPLUS: &[u8] = include_bytes!("../examples/assets/fonts/Mplus1Code-Regular.ttf");
    const COLRV1: &[u8] = include_bytes!("backend/testdata/noto_colrv1_subset.ttf");
    const CBDT: &[u8] = include_bytes!("backend/testdata/noto_cbdt_subset.ttf");

    fn cell(symbol: &str) -> Cell {
        let mut cell = Cell::default();
        cell.set_symbol(symbol);
        cell
    }

    #[test]
    fn emoji_presentation_is_true_for_emoji_sequences() {
        for symbol in [
            "\u{1F600}",
            "\u{1F44D}\u{1F3FD}",
            "\u{1F9D1}\u{200D}\u{1F4BB}",
            "\u{1F1EF}\u{1F1F5}",
            "\u{2764}\u{FE0F}",
            "1\u{FE0F}\u{20E3}",
            "\u{1F3F3}\u{FE0F}\u{200D}\u{1F308}",
            "\u{1F525}",
            "\u{1F3FD}",
            "\u{1F600}\u{301}",
        ] {
            assert!(emoji_presentation(symbol), "{symbol:?}");
        }
    }

    #[test]
    fn emoji_presentation_is_false_for_text() {
        for symbol in [
            "1",
            "#",
            "\u{A9}",
            "\u{2764}",
            "\u{2600}",
            "\u{2764}\u{FE0E}",
            "\u{1F600}\u{FE0E}",
            "\u{E9}",
            "e\u{301}",
            "1\u{301}",
            "\u{3042}",
            "\u{1F1EF}",
            "a",
        ] {
            assert!(!emoji_presentation(symbol), "{symbol:?}");
        }
    }

    #[test]
    fn emoji_fonts_win_emoji_presentation_and_text_keeps_its_font() {
        for data in [COLRV1, CBDT] {
            let emoji = Font::new(data).expect("emoji fixture parses");
            let emoji_id = emoji.id();
            let mplus = Font::new(MPLUS).expect("M+ parses");
            let mplus_id = mplus.id();
            let mut fonts = Fonts::new(mplus, 16);
            fonts.add_emoji_fonts([emoji]);

            for symbol in ["\u{1F600}", "\u{2764}\u{FE0F}"] {
                let (font, fake_bold, fake_italic) = fonts.font_for_cell(&cell(symbol));
                assert_eq!(font.id(), emoji_id, "{symbol:?}");
                assert!(!fake_bold && !fake_italic);
                assert!(fonts.is_emoji_font(emoji_id));
                assert_eq!(fonts.font_by_id(emoji_id).id(), emoji_id);
            }
            assert_eq!(fonts.font_for_cell(&cell("1")).0.id(), mplus_id);
            assert!(!fonts.is_emoji_font(mplus_id));
        }
    }

    #[test]
    fn an_emoji_font_is_the_fallback_for_a_char_no_text_font_has() {
        let emoji = Font::new(COLRV1).expect("emoji fixture parses");
        let emoji_id = emoji.id();
        let mut fonts = Fonts::new(Font::new(MPLUS).expect("M+ parses"), 16);
        fonts.add_emoji_fonts([emoji]);
        let sun = cell("\u{2600}");
        assert!(!emoji_presentation(sun.symbol()));
        assert!(fonts.last_resort.font().glyph_index('\u{2600}').is_none());
        assert_eq!(fonts.font_for_cell(&sun).0.id(), emoji_id);
        assert_eq!(
            fonts.font_for_cell(&cell("#")).0.id(),
            fonts.last_resort.id()
        );
    }

    #[test]
    fn adding_emoji_fonts_does_not_change_the_cell_size() {
        for data in [COLRV1, CBDT] {
            let mut fonts = Fonts::new(Font::new(MPLUS).expect("M+ parses"), 16);
            let (width, height, count) = (fonts.min_width_px(), fonts.height_px(), fonts.count());
            fonts.add_emoji_fonts([Font::new(data).expect("emoji fixture parses")]);
            assert_eq!(fonts.min_width_px(), width);
            assert_eq!(fonts.height_px(), height);
            assert_eq!(fonts.count(), count + 1, "the plan cache is sized for it");
        }
    }
}
