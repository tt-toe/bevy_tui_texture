//! Caches for the per-row text pipeline, shared by every terminal using the
//! same `Fonts` (they live in `SharedFontCpuState`).
//!
//! Shaping with rustybuzz is by far the most expensive step of a flush, and
//! a terminal shows the same few symbols over and over. [`CellShapeCache`]
//! keeps the shaped glyphs of each cell symbol, so rustybuzz runs once per
//! distinct symbol; [`FontChoiceCache`] does the same for the per-cell font
//! fallback probe that precedes shaping.

use bevy::platform::collections::HashMap;

use crate::fonts::Font;

/// One shaped glyph - the subset of rustybuzz's output the terminal grid
/// uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShapedGlyph {
    pub(crate) glyph_id: u32,
    /// Byte offset of the glyph's cluster within the shaped text.
    pub(crate) cluster: u32,
    pub(crate) x_advance: i32,
    pub(crate) x_offset: i32,
}

/// Shaped glyphs of single cells, keyed by `(font, cell symbol)` - the
/// cache behind [`Shaping::Cell`](crate::fonts::Shaping::Cell). A cell's symbol
/// is one grapheme cluster, so the number of distinct keys is bounded by the
/// characters an application displays, not by how they are arranged: text
/// never seen before costs the same as text seen every frame.
#[derive(Default)]
pub(crate) struct CellShapeCache {
    fonts: HashMap<u64, FontCellShapes>,
}

impl CellShapeCache {
    /// One font's cells. Looked up once per font run, so the per-cell work
    /// below involves no font hashing.
    pub(crate) fn font(&mut self, font: &Font) -> &mut FontCellShapes {
        self.fonts.entry(font.id()).or_default()
    }
}

pub(crate) struct FontCellShapes {
    /// Direct-mapped front for the usual cell: a single `char` shaping to
    /// a single glyph, found without hashing a string. Two chars sharing a
    /// slot just take turns - `all` stays the source of truth.
    single: Box<[Option<(char, ShapedGlyph)>; Self::SINGLE_SLOTS]>,
    all: HashMap<Box<str>, Box<[ShapedGlyph]>>,
}

impl Default for FontCellShapes {
    fn default() -> Self {
        Self {
            single: Box::new([None; Self::SINGLE_SLOTS]),
            all: HashMap::default(),
        }
    }
}

impl FontCellShapes {
    /// A power of two; ASCII maps onto its own code points.
    const SINGLE_SLOTS: usize = 2048;
    /// Distinct symbols kept per font. Far above what real content shows;
    /// reaching it (endless novel combining sequences) drops the cache
    /// rather than letting it grow without bound.
    const MAX_SYMBOLS: usize = 32 * 1024;

    /// The shaped glyphs of `symbol`, calling `shape` only the first time
    /// this symbol is seen in this font.
    pub(crate) fn get_or_shape(
        &mut self,
        symbol: &str,
        shape: impl FnOnce() -> Vec<ShapedGlyph>,
    ) -> &[ShapedGlyph] {
        let mut chars = symbol.chars();
        let single = match (chars.next(), chars.next()) {
            (Some(ch), None) => Some((ch, ch as usize & (Self::SINGLE_SLOTS - 1))),
            _ => None,
        };
        if let Some((ch, slot)) = single
            && self.single[slot].is_some_and(|(cached, _)| cached == ch)
        {
            return self.single[slot]
                .as_ref()
                .map(|(_, glyph)| std::slice::from_ref(glyph))
                .expect("checked above");
        }
        if !self.all.contains_key(symbol) {
            if self.all.len() >= Self::MAX_SYMBOLS {
                self.all.clear();
            }
            self.all.insert(symbol.into(), shape().into_boxed_slice());
        }
        let glyphs = &self.all[symbol];
        if let (Some((ch, slot)), [glyph]) = (single, &**glyphs) {
            self.single[slot] = Some((ch, *glyph));
        }
        glyphs
    }
}

/// Which font renders a cell, and with which fake styling - the result of
/// `Fonts::font_for_cell`, stored by font id so it can outlive the borrow.
type FontChoice = (u64, bool, bool);

/// Memoizes `Fonts::font_for_cell` for single-`char` symbols (practically
/// every cell). The uncached probe walks the fallback chain doing a cmap
/// lookup per font, for every cell of every reshaped row; a `Fonts` is
/// immutable once shared, so its answer for a given `(char, bold, italic)`
/// never changes.
pub(crate) struct FontChoiceCache {
    /// Direct-mapped by `char | style << 7` - no hashing for ASCII.
    ascii: Box<[Option<FontChoice>; 128 * 4]>,
    other: HashMap<(char, u8), FontChoice>,
}

impl FontChoiceCache {
    pub(crate) fn new() -> Self {
        Self {
            ascii: Box::new([None; 128 * 4]),
            other: HashMap::default(),
        }
    }

    /// `style` is `bold | italic << 1`.
    pub(crate) fn get_or_insert_with(
        &mut self,
        ch: char,
        style: u8,
        choose: impl FnOnce() -> FontChoice,
    ) -> FontChoice {
        debug_assert!(style < 4);
        if ch.is_ascii() {
            *self.ascii[ch as usize | (style as usize) << 7].get_or_insert_with(choose)
        } else {
            *self.other.entry((ch, style)).or_insert_with(choose)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MPLUS: &[u8] = include_bytes!("../../examples/assets/fonts/Mplus1Code-Regular.ttf");

    fn glyph(id: u32) -> Vec<ShapedGlyph> {
        vec![ShapedGlyph {
            glyph_id: id,
            cluster: 0,
            x_advance: 0,
            x_offset: 0,
        }]
    }

    #[test]
    fn a_cell_is_shaped_once_per_font() {
        let font = Font::new(MPLUS).expect("font parses");
        let other = Font::new(MPLUS).expect("font parses"); // fresh id
        let mut cache = CellShapeCache::default();
        for symbol in ["a", "あ", "e\u{301}"] {
            assert_eq!(
                cache.font(&font).get_or_shape(symbol, || glyph(1))[0].glyph_id,
                1
            );
            let again = cache
                .font(&font)
                .get_or_shape(symbol, || panic!("must be a cache hit"));
            assert_eq!(again[0].glyph_id, 1);
            assert_eq!(
                cache.font(&other).get_or_shape(symbol, || glyph(2))[0].glyph_id,
                2
            );
        }
    }

    /// A single char shaping to something other than one glyph still
    /// round-trips (it just isn't direct-mapped).
    #[test]
    fn an_ascii_cell_may_shape_to_no_glyph_or_several() {
        let font = Font::new(MPLUS).expect("font parses");
        let mut cache = CellShapeCache::default();
        assert!(cache.font(&font).get_or_shape("a", Vec::new).is_empty());
        assert!(
            cache
                .font(&font)
                .get_or_shape("a", || panic!("cached"))
                .is_empty()
        );
        let two = || [glyph(1), glyph(2)].concat();
        assert_eq!(cache.font(&font).get_or_shape("b", two).len(), 2);
        assert_eq!(
            cache
                .font(&font)
                .get_or_shape("b", || panic!("cached"))
                .len(),
            2
        );
    }

    /// Chars sharing a direct-mapped slot evict each other there, never
    /// from the cache.
    #[test]
    fn colliding_single_chars_stay_cached() {
        let font = Font::new(MPLUS).expect("font parses");
        let mut cache = CellShapeCache::default();
        let (a, b) = ("a", "\u{861}"); // 0x61 and 0x861 share a slot
        assert_eq!(
            cache.font(&font).get_or_shape(a, || glyph(1))[0].glyph_id,
            1
        );
        assert_eq!(
            cache.font(&font).get_or_shape(b, || glyph(2))[0].glyph_id,
            2
        );
        for _ in 0..2 {
            let cells = cache.font(&font);
            assert_eq!(cells.get_or_shape(a, || panic!("cached"))[0].glyph_id, 1);
            assert_eq!(cells.get_or_shape(b, || panic!("cached"))[0].glyph_id, 2);
        }
    }

    #[test]
    fn font_choices_are_memoized_per_char_and_style() {
        let mut cache = FontChoiceCache::new();
        assert_eq!(cache.get_or_insert_with('a', 0, || (1, false, false)).0, 1);
        assert_eq!(cache.get_or_insert_with('a', 0, || panic!("cached")).0, 1);
        assert_eq!(cache.get_or_insert_with('a', 1, || (2, true, false)).0, 2);
        assert_eq!(cache.get_or_insert_with('あ', 0, || (3, false, false)).0, 3);
        assert_eq!(cache.get_or_insert_with('あ', 0, || panic!("cached")).0, 3);
    }
}
