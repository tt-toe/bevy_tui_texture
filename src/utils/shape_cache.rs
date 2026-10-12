//! Caches for the per-row text pipeline, shared by every terminal using the
//! same `Fonts` (they live in `SharedFontCpuState`).
//!
//! Shaping a row with rustybuzz is by far the most expensive step of a
//! flush, yet terminal content is overwhelmingly repetitive: a row whose
//! colors changed still has the same text, and scrolled content is the same
//! text on a different row. [`ShapeCache`] keys shaped runs by their text so
//! both cases skip rustybuzz entirely; [`FontChoiceCache`] does the same for
//! the per-cell font fallback probe that precedes shaping.

use std::hash::BuildHasher;
use std::num::NonZeroUsize;

use bevy::platform::collections::HashMap;
use bevy::platform::hash::FixedHasher;
use evictor::Lru;

use crate::fonts::Font;

/// One shaped glyph of a run - the subset of rustybuzz's output the
/// terminal grid uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShapedGlyph {
    pub(crate) glyph_id: u32,
    /// Byte offset of the glyph's cluster, relative to the run's start.
    pub(crate) cluster: u32,
    pub(crate) x_advance: i32,
    pub(crate) x_offset: i32,
}

struct ShapedRun {
    /// The exact run text, compared on every hit: the LRU is keyed by a
    /// 64-bit hash, and a collision must degrade to a reshape, never to
    /// another run's glyphs.
    text: Box<str>,
    font: u64,
    glyphs: Box<[ShapedGlyph]>,
}

/// LRU of shaped runs keyed by `(font, text)`.
pub(crate) struct ShapeCache {
    lru: Lru<u64, ShapedRun>,
}

// SAFETY: evictor 0.7's `Lru` exclusively owns its arena (the raw pointers
// it holds only point into that arena) and has no thread-affine state, so
// moving it to another thread is sound for `Send` keys/values. Only `Send`
// is asserted - shared access always goes through the `Mutex` in
// `Fonts::with_shared_cpu_state`.
unsafe impl Send for ShapeCache {}

impl ShapeCache {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            lru: Lru::new(NonZeroUsize::new(capacity.max(1)).expect("capacity is non-zero")),
        }
    }

    /// The shaped glyphs of `text` in `font`, calling `shape` only when
    /// this exact run isn't cached.
    pub(crate) fn get_or_shape(
        &mut self,
        font: &Font,
        text: &str,
        shape: impl FnOnce() -> Vec<ShapedGlyph>,
    ) -> &[ShapedGlyph] {
        let key = FixedHasher.hash_one((font.id(), text));
        let hit = self
            .lru
            .peek(&key)
            .is_some_and(|run| run.font == font.id() && &*run.text == text);
        if !hit {
            // Also the hash-collision path: replaces the colliding entry.
            self.lru.insert(
                key,
                ShapedRun {
                    text: text.into(),
                    font: font.id(),
                    glyphs: shape().into_boxed_slice(),
                },
            );
        }
        // `get` (not `peek`) so a hit counts as a use.
        &self.lru.get(&key).expect("present or just inserted").glyphs
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
    fn a_cached_run_is_not_reshaped() {
        let font = Font::new(MPLUS).expect("font parses");
        let mut cache = ShapeCache::new(8);
        assert_eq!(cache.get_or_shape(&font, "abc", || glyph(1))[0].glyph_id, 1);
        let again = cache.get_or_shape(&font, "abc", || panic!("must be a cache hit"));
        assert_eq!(again[0].glyph_id, 1);
    }

    #[test]
    fn runs_are_keyed_by_text_and_font() {
        let font = Font::new(MPLUS).expect("font parses");
        let other = Font::new(MPLUS).expect("font parses"); // fresh id
        let mut cache = ShapeCache::new(8);
        cache.get_or_shape(&font, "abc", || glyph(1));
        assert_eq!(cache.get_or_shape(&font, "abd", || glyph(2))[0].glyph_id, 2);
        assert_eq!(
            cache.get_or_shape(&other, "abc", || glyph(3))[0].glyph_id,
            3
        );
        assert_eq!(cache.get_or_shape(&font, "abc", || glyph(9))[0].glyph_id, 1);
    }

    #[test]
    fn the_least_recently_used_run_is_evicted() {
        let font = Font::new(MPLUS).expect("font parses");
        let mut cache = ShapeCache::new(2);
        cache.get_or_shape(&font, "a", || glyph(1));
        cache.get_or_shape(&font, "b", || glyph(2));
        cache.get_or_shape(&font, "a", || glyph(9)); // refresh "a"
        cache.get_or_shape(&font, "c", || glyph(3)); // evicts "b"
        assert_eq!(cache.get_or_shape(&font, "a", || glyph(9))[0].glyph_id, 1);
        assert_eq!(cache.get_or_shape(&font, "b", || glyph(7))[0].glyph_id, 7);
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
