use std::num::NonZeroUsize;
use std::ops::Deref;

use evictor::Lru;
use tracing::info;
use ratatui::style::Modifier;

use crate::Fonts;

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub(crate) struct Key {
    pub(crate) style: Modifier,
    pub(crate) glyph: u32,
    pub(crate) font: u64,
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

#[derive(Debug)]
pub(crate) struct Atlas {
    lru: Lru<Key, CacheRect>,
    width: u32,

    entry_width: u32,
    entry_height: u32,

    next_entry: u32,
    max_entries: u32,

    /// Bumped every time a slot is reassigned to a different key (eviction
    /// in `get`, or a full `clear`) - see IMPROVEMENT.md A2. A cached
    /// row's UV rects are only trustworthy as long as no reassignment has
    /// happened since they were recorded; comparing this counter is how
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
            lru: Lru::new(
                NonZeroUsize::new(max_entries as usize).expect("Max entries must be non-zero"),
            ),
            width,
            entry_width,
            entry_height,
            next_entry: 0,
            max_entries,
            generation: 0,
        }
    }

    /// Current generation counter - see the field doc on [`Atlas::generation`].
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn try_get(&mut self, key: &Key) -> Option<Entry> {
        self.lru.get(key).copied().map(Entry::Cached)
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

        self.try_get(key).unwrap_or_else(|| {
            let rect = if self.next_entry == self.max_entries {
                // Reassigning an existing slot to a new key - any row
                // geometry caching that slot's old UV rect is now stale.
                self.generation += 1;
                self.lru.pop().expect("Atlas has zero max entries!").1
            } else {
                let entry = self.next_entry;
                self.next_entry += 1;
                self.slot_to_rect(entry, width)
            };

            self.lru.insert(*key, rect);
            Entry::Uncached(rect)
        })
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
                },
                char_width,
                char_height,
            );
        }

        let last_key = Key {
            style: Modifier::default(),
            glyph: u32::MAX,
            font: u32::MAX as _,
        };

        let last_inserted = atlas.get(&last_key, char_width, char_height);
        let post_insertion = atlas.get(&last_key, char_width, char_height);

        assert_eq!(*last_inserted, *post_insertion);
    }
}
