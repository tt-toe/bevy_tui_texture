use std::ops::Deref;

use bevy::platform::collections::HashMap;
use ratatui::style::Modifier;
use tracing::info;

use crate::Fonts;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) struct Key {
    pub(crate) style: Modifier,
    /// Glyph id, or the codepoint of a procedurally drawn glyph (< 2^21).
    pub(crate) glyph: u32,
    pub(crate) font: u64,
    /// Cells the glyph is rasterized across: a symbol drawn into one cell
    /// and the same symbol spread over two are different images.
    pub(crate) cells: u8,
}

/// One hasher write per lookup instead of one per field: the font id is
/// already a random 64-bit hash, so the small fields are folded into it,
/// each in its own bits. Keys that differ only in their font still differ
/// after the XOR; a coincidence across fonts is a mere collision.
impl std::hash::Hash for Key {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let small =
            u64::from(self.style.bits()) << 48 | u64::from(self.glyph) << 8 | u64::from(self.cells);
        state.write_u64(self.font ^ small);
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) struct CacheRect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Entry {
    Cached(CacheRect),
    Uncached(CacheRect),
}

impl Entry {
    pub(crate) fn cached(&self) -> bool {
        matches!(self, Entry::Cached(_))
    }
}

impl Deref for Entry {
    type Target = CacheRect;

    fn deref(&self) -> &Self::Target {
        let (Entry::Cached(entry) | Entry::Uncached(entry)) = self;
        entry
    }
}

/// One atlas slot's bookkeeping: which glyph currently lives there, and
/// the last flush that used it.
#[derive(Debug, Clone, Copy)]
struct Slot {
    key: Key,
    last_used: u64,
}

/// CPU-side index of the glyph atlas: which fixed-size slot each glyph
/// occupies, and which slot to reuse once the atlas is full.
///
/// A lookup is the hot path of every flush - it runs once per glyph of
/// every reshaped row - so a hit costs one hash probe plus one store: the
/// slot is stamped with the current flush's epoch instead of being relinked
/// to the front of a recency list. Eviction (the rare path, only once the
/// atlas is full) is a clock sweep over those stamps.
#[derive(Debug)]
pub(crate) struct Atlas {
    index: HashMap<Key, u32>,
    /// Allocated slots, by slot number.
    slots: Vec<Slot>,
    width: u32,

    entry_width: u32,
    entry_height: u32,

    max_entries: u32,

    /// Advanced by [`Atlas::begin_flush`]; what a used slot is stamped with.
    epoch: u64,
    /// Where the next eviction sweep starts.
    hand: u32,

    /// Bumped every time a slot is reassigned to a different key. A cached
    /// row's atlas rects are only trustworthy as long as no reassignment
    /// has happened since they were recorded; comparing this counter is how
    /// `BevyTerminalBackend::flush` (the only reader) detects that.
    generation: u64,
}

impl Atlas {
    pub(crate) fn new(fonts: &Fonts, width: u32, height: u32) -> Self {
        let entry_width = fonts.min_width_px() * 2;
        let entry_height = fonts.height_px();
        let max_entries = ((width / entry_width) * (height / entry_height)).max(1);
        info!(
            "Glyph atlas: {width}x{height}px in use, entries {entry_width}x{entry_height}px, capacity {max_entries}"
        );

        Atlas {
            index: HashMap::default(),
            slots: Vec::new(),
            width,
            entry_width,
            entry_height,
            max_entries,
            epoch: 0,
            hand: 0,
            generation: 0,
        }
    }

    /// Current generation counter - see the field doc on [`Atlas::generation`].
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// Starts a new flush: every glyph looked up from here on counts as in
    /// use by this flush, and is the last thing an eviction will pick.
    pub(crate) fn begin_flush(&mut self) {
        self.epoch += 1;
    }

    pub(crate) fn get(&mut self, key: &Key, width: u32, height: u32) -> Entry {
        debug_assert_eq!(
            self.entry_height, height,
            "Internal height not equal to provided height - entry size is fixed at Atlas::new time"
        );
        debug_assert_eq!(
            self.entry_width % width,
            0,
            "Internal width not a multiple of provided width - entry size is fixed at Atlas::new time"
        );

        if let Some(&slot) = self.index.get(key) {
            self.slots[slot as usize].last_used = self.epoch;
            return Entry::Cached(self.slot_to_rect(slot, width));
        }

        let slot = if (self.slots.len() as u32) < self.max_entries {
            self.slots.push(Slot {
                key: *key,
                last_used: self.epoch,
            });
            self.slots.len() as u32 - 1
        } else {
            // Reassigning an existing slot to a new key - any row geometry
            // caching that slot's old rect is now stale.
            self.generation += 1;
            let slot = self.evict();
            self.index.remove(&self.slots[slot as usize].key);
            self.slots[slot as usize] = Slot {
                key: *key,
                last_used: self.epoch,
            };
            slot
        };
        self.index.insert(*key, slot);
        Entry::Uncached(self.slot_to_rect(slot, width))
    }

    /// Picks the slot to reuse, sweeping from the clock hand: preferably
    /// one no flush has touched for a while, otherwise any slot the current
    /// flush hasn't used. Only when every slot is in use by the current
    /// flush (more distinct glyphs on screen than the atlas holds) does it
    /// take one of those - the generation bump in `get` makes the affected
    /// rows reshape.
    fn evict(&mut self) -> u32 {
        let count = self.slots.len() as u32;
        // "Stale" = untouched by this flush and the one before it, so a
        // glyph that merely skipped one frame isn't the first to go.
        for stale_before in [self.epoch.saturating_sub(1), self.epoch] {
            for step in 0..count {
                let slot = (self.hand + step) % count;
                if self.slots[slot as usize].last_used < stale_before {
                    self.hand = (slot + 1) % count;
                    return slot;
                }
            }
        }
        let slot = self.hand % count;
        self.hand = (slot + 1) % count;
        slot
    }

    fn slot_to_rect(&self, slot: u32, width: u32) -> CacheRect {
        let x = slot % (self.width / self.entry_width) * self.entry_width;
        let y = slot / (self.width / self.entry_width) * self.entry_height;
        CacheRect {
            x,
            y,
            width,
            height: self.entry_height,
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::Modifier;

    use crate::Font;
    use crate::Fonts;
    use crate::utils::text_atlas::Atlas;
    use crate::utils::text_atlas::Key;

    #[test]
    fn reuse() {
        let fonts = Fonts::new(
            Font::new(include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/examples/assets/fonts/Mplus1Code-Regular.ttf"
            )))
            .unwrap(),
            24,
        );
        let mut atlas = Atlas::new(&fonts, 24, 24);

        // Get the correct width and height from the fonts
        let char_width = fonts.min_width_px();
        let char_height = fonts.height_px();

        for idx in 0..atlas.max_entries {
            atlas.get(
                &Key {
                    style: Modifier::default(),
                    glyph: idx as _,
                    font: idx as _,
                    cells: 1,
                },
                char_width,
                char_height,
            );
        }

        let last_key = Key {
            style: Modifier::default(),
            glyph: u32::MAX,
            font: u32::MAX as _,
            cells: 1,
        };

        let last_inserted = atlas.get(&last_key, char_width, char_height);
        let post_insertion = atlas.get(&last_key, char_width, char_height);

        assert_eq!(*last_inserted, *post_insertion);
    }

    fn key(glyph: u32) -> Key {
        Key {
            style: Modifier::default(),
            glyph,
            font: 0,
            cells: 1,
        }
    }

    fn small_atlas() -> (Atlas, u32, u32) {
        let fonts = Fonts::new(
            Font::new(include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/examples/assets/fonts/Mplus1Code-Regular.ttf"
            )))
            .unwrap(),
            24,
        );
        let (w, h) = (fonts.min_width_px(), fonts.height_px());
        // Room for exactly 4 entries (2 x 2 slots of `2w` x `h`).
        (Atlas::new(&fonts, w * 4, h * 2), w, h)
    }

    #[test]
    fn a_full_atlas_evicts_a_glyph_no_recent_flush_used() {
        let (mut atlas, w, h) = small_atlas();
        assert_eq!(atlas.max_entries, 4);

        atlas.begin_flush();
        let rects: Vec<_> = (0..4).map(|glyph| *atlas.get(&key(glyph), w, h)).collect();
        // Two more flushes keep using everything except glyph 2.
        for _ in 0..2 {
            atlas.begin_flush();
            for glyph in [0, 1, 3] {
                assert!(atlas.get(&key(glyph), w, h).cached());
            }
        }

        let generation = atlas.generation();
        let new = atlas.get(&key(99), w, h);
        assert!(!new.cached());
        assert_eq!(*new, rects[2], "the new glyph takes the stale slot");
        assert_eq!(atlas.generation(), generation + 1);
        assert!(!atlas.get(&key(2), w, h).cached(), "glyph 2 was evicted");
    }

    #[test]
    fn glyphs_used_by_the_current_flush_are_evicted_last() {
        let (mut atlas, w, h) = small_atlas();
        atlas.begin_flush();
        let rects: Vec<_> = (0..4).map(|glyph| *atlas.get(&key(glyph), w, h)).collect();

        atlas.begin_flush();
        for glyph in [0, 1, 2] {
            atlas.get(&key(glyph), w, h);
        }
        // Only glyph 3 is unused by this flush - it must be the one replaced.
        assert_eq!(*atlas.get(&key(50), w, h), rects[3]);
        for glyph in [0, 1, 2] {
            assert!(atlas.get(&key(glyph), w, h).cached());
        }
    }
}
