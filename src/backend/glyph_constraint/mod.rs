//! Fitting symbol glyphs (Nerd Font icons, Powerline extras, wide-drawn
//! ambiguous-width symbols) to the terminal's cell grid.
//!
//! A text glyph is drawn the way its font positions it. A symbol is not:
//! the font it comes from is usually a fallback whose em and advance have
//! nothing to do with this terminal's cells (Symbols Nerd Font's icons are
//! about one em wide, two to three cells of a typical primary font). Nerd
//! Fonts' font-patcher solves the same problem when it patches icons into a
//! font: every symbol set comes with rules for how to scale and align its
//! glyphs into the target font's cell. This module applies those rules at
//! render time, against this terminal's cells, so the same icons look the
//! same whatever font they come from (patched or not, Mono or not).
//!
//! - [`GlyphConstraint`] is the rule: how to size the glyph, how to align
//!   it, how many cells it may use. The resolution math
//!   ([`GlyphConstraint::constrain`]) is a port of Ghostty's
//!   `Glyph.RenderOptions.Constraint` (MIT, see NOTICE), which mirrors
//!   font-patcher.
//! - `nerd_font.rs` is font-patcher's own rule set, generated from it by
//!   `tools/nerd_font_constraints.py`. Symbols it doesn't cover get
//!   [`GlyphConstraint::FIT`]: shrink to fit if too big, otherwise
//!   untouched.
//! - [`CellClass`] says whether a cell is a symbol and how many cells it
//!   may take; it is decided once per `(char, style)` together with the
//!   cell's font. A symbol may use two cells when the next cell is blank -
//!   see [`symbol_span`].

mod nerd_font;

use unicode_width::UnicodeWidthChar;

/// How a constrained glyph is scaled into its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Size {
    /// Keep the glyph's size.
    None,
    /// Scale down to fit if needed, preserving aspect ratio.
    Fit,
    /// Scale up or down to fit the box exactly, preserving aspect ratio.
    Cover,
    /// Like [`Size::Fit`], but scale up to cover at least one cell; a
    /// glyph between one cell and the box keeps its size (font-patcher's
    /// `pa` rule).
    FitCover1,
    /// Stretch to the box in both directions, ignoring aspect ratio.
    Stretch,
}

/// How a constrained glyph is aligned on one axis of its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Align {
    /// Keep the glyph where it is, moved only as far as needed to stay
    /// inside the box.
    None,
    /// Left / bottom edge of the box.
    Start,
    /// Right / top edge of the box.
    End,
    /// Centered - horizontally within the *first* cell even for a two-cell
    /// box, so a glyph wider than a cell starts at the cell's left edge
    /// (font-patcher's `c`).
    Center1,
}

/// The height a constrained glyph is sized against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Height {
    /// The full cell height.
    Cell,
    /// The icon height: the cell height for a two-cell box, a bit less for
    /// one cell (see [`CellMetrics::icon_height_single`]).
    Icon,
}

/// A rule for scaling and aligning a glyph into the cells it occupies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GlyphConstraint {
    pub(crate) size: Size,
    pub(crate) height: Height,
    /// The most cells the glyph may use (1 or 2).
    pub(crate) max_cells: u8,
    pub(crate) align_x: Align,
    pub(crate) align_y: Align,
    /// Padding inside the box: top, right, bottom, left, as fractions of
    /// the cell height (top/bottom) or width (left/right). Negative values
    /// let the glyph overlap its neighbours.
    pub(crate) pad: [f32; 4],
    /// The glyph's bounding box relative to that of its scale group:
    /// x, y, width, height. Glyphs of one group are scaled by the same
    /// factor and aligned as one, which keeps their relative sizes.
    pub(crate) group: [f32; 4],
    /// Widest aspect ratio (width / height) a stretch may produce; 0 for
    /// no limit.
    pub(crate) max_xy_ratio: f32,
}

impl GlyphConstraint {
    /// The rule for a symbol font-patcher has no rule for: shrink to fit
    /// the box if needed, otherwise leave the glyph alone.
    pub(crate) const FIT: Self = Self {
        size: Size::Fit,
        height: Height::Cell,
        max_cells: 2,
        align_x: Align::None,
        align_y: Align::None,
        pad: [0.0; 4],
        group: [0.0, 0.0, 1.0, 1.0],
        max_xy_ratio: 0.0,
    };

    /// font-patcher's rule for `ch`, if it patches that codepoint.
    pub(crate) fn nerd_font(ch: char) -> Option<&'static Self> {
        let cp = ch as u32;
        let ranges = &nerd_font::RANGES;
        let idx = ranges.partition_point(|&(_, last, _)| last < cp);
        let &(first, _, constraint) = ranges.get(idx)?;
        (first <= cp).then(|| &nerd_font::CONSTRAINTS[constraint as usize])
    }

    /// The rule for symbol `ch`: font-patcher's, or [`Self::FIT`].
    pub(crate) fn for_symbol(ch: char) -> &'static Self {
        Self::nerd_font(ch).unwrap_or(&Self::FIT)
    }

    /// Where a glyph whose ink is `glyph` (pixels, y up from the bottom of
    /// the cell) lands when constrained into `cells` cells.
    pub(crate) fn constrain(
        &self,
        glyph: GlyphRect,
        metrics: &CellMetrics,
        cells: u32,
    ) -> GlyphRect {
        if self.size == Size::None && self.align_x == Align::None && self.align_y == Align::None {
            return glyph;
        }
        let mut c = *self;
        if c.size == Size::Stretch {
            // Negative padding on a stretched glyph only guards against
            // rounding gaps between the face and the pixel grid; there is
            // no such gap here (the face is the cell).
            c.pad = c.pad.map(|p| p.max(0.0));
        }
        c.constrain_inner(glyph, metrics, cells)
    }

    fn constrain_inner(&self, glyph: GlyphRect, m: &CellMetrics, cells: u32) -> GlyphRect {
        // Never stretch across two cells for an unusually wide face (as
        // font-patcher).
        let cells = if self.size == Size::Stretch && m.cell_width > 0.9 * m.cell_height {
            1
        } else {
            cells.min(self.max_cells as u32).max(1)
        };

        let [rel_x, rel_y, rel_w, rel_h] = self.group;
        let mut group = GlyphRect {
            width: glyph.width / rel_w,
            height: glyph.height / rel_h,
            x: 0.0,
            y: 0.0,
        };
        group.x = glyph.x - group.width * rel_x;
        group.y = glyph.y - group.height * rel_y;

        // Scale about the group's center, then align.
        let (width_factor, height_factor) = self.scale_factors(&group, m, cells);
        let center_x = group.x + group.width / 2.0;
        let center_y = group.y + group.height / 2.0;
        group.width *= width_factor;
        group.height *= height_factor;
        group.x = center_x - group.width / 2.0;
        group.y = center_y - group.height / 2.0;
        group.y = self.aligned_y(&group, m);
        group.x = self.aligned_x(&group, m, cells);

        GlyphRect {
            width: width_factor * glyph.width,
            height: height_factor * glyph.height,
            x: group.x + group.width * rel_x,
            y: group.y + group.height * rel_y,
        }
    }

    fn scale_factors(&self, group: &GlyphRect, m: &CellMetrics, cells: u32) -> (f32, f32) {
        if self.size == Size::None {
            return (1.0, 1.0);
        }
        let [pad_top, pad_right, pad_bottom, pad_left] = self.pad;
        let multi_cell = cells > 1;
        let target_width = (cells as f32 - (pad_left + pad_right)) * m.cell_width;
        let target_height = (1.0 - (pad_bottom + pad_top))
            * match self.height {
                Height::Cell => m.cell_height,
                Height::Icon if multi_cell => m.cell_height,
                Height::Icon => m.icon_height_single,
            };

        let mut width_factor = target_width / group.width;
        let mut height_factor = target_height / group.height;
        match self.size {
            Size::None => unreachable!(),
            Size::Fit => {
                height_factor = height_factor.min(width_factor).min(1.0);
                width_factor = height_factor;
            }
            Size::Cover => {
                height_factor = height_factor.min(width_factor);
                width_factor = height_factor;
            }
            Size::FitCover1 => {
                height_factor = height_factor.min(width_factor);
                if multi_cell && height_factor > 1.0 {
                    // As big as it would be in one cell - a glyph doesn't
                    // grow just because a blank cell follows it.
                    let (_, single) = self.scale_factors(group, m, 1);
                    height_factor = single.max(1.0);
                }
                width_factor = height_factor;
            }
            Size::Stretch => {}
        }

        if self.max_xy_ratio > 0.0
            && group.width * width_factor > group.height * height_factor * self.max_xy_ratio
        {
            width_factor = group.height * height_factor * self.max_xy_ratio / group.width;
        }
        (width_factor, height_factor)
    }

    fn aligned_y(&self, group: &GlyphRect, m: &CellMetrics) -> f32 {
        if self.size == Size::None && self.align_y == Align::None {
            return group.y;
        }
        let [pad_top, _, pad_bottom, _] = self.pad;
        let start = pad_bottom * m.cell_height;
        let end = m.cell_height - group.height - pad_top * m.cell_height;
        let center = (start + end) / 2.0;
        match self.align_y {
            // Every size rule implies staying inside the padded cell; a
            // glyph too tall for it is centered.
            Align::None if end < start => center,
            Align::None => group.y.min(end).max(start),
            Align::Start => start,
            Align::End => end,
            Align::Center1 => center,
        }
    }

    fn aligned_x(&self, group: &GlyphRect, m: &CellMetrics, cells: u32) -> f32 {
        if self.size == Size::None && self.align_x == Align::None {
            return group.x;
        }
        let [_, pad_right, _, pad_left] = self.pad;
        let start = pad_left * m.cell_width;
        let end = cells as f32 * m.cell_width - group.width - pad_right * m.cell_width;
        match self.align_x {
            // The left edge wins when the group is too wide.
            Align::None => group.x.min(end).max(start),
            Align::Start => start,
            Align::End => end.max(start),
            Align::Center1 => {
                let end1 = m.cell_width - group.width - pad_right * m.cell_width;
                start.max((start + end1) / 2.0)
            }
        }
    }
}

/// A glyph's ink box in pixels: `x` from the cell's left edge, `y` up from
/// the cell's bottom edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GlyphRect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

/// The cell grid constraints resolve against, in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CellMetrics {
    pub(crate) cell_width: f32,
    pub(crate) cell_height: f32,
    /// Icon height for a one-cell box: `(2 * cap height + cell height) / 3`
    /// of the primary font, font-patcher's heuristic.
    pub(crate) icon_height_single: f32,
}

/// Whether a cell's glyph is a symbol, and how many cells it may take -
/// decided once per `(char, style)` alongside the font choice (see
/// `Fonts::font_for_cell_cached`), so reshaping a row costs no more than
/// reading it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct CellClass {
    /// 0: not a symbol, drawn as before. 1 or 2: a symbol, constrained into
    /// at most this many cells.
    pub(crate) max_cells: u8,
    /// A terminal graphics element (box drawing, blocks, Powerline
    /// separators): a symbol before it doesn't stop the next one from
    /// spreading.
    pub(crate) graphics: bool,
}

impl CellClass {
    pub(crate) const TEXT: Self = Self {
        max_cells: 0,
        graphics: false,
    };

    pub(crate) fn is_symbol(self) -> bool {
        self.max_cells > 0
    }

    /// Classifies `ch` as drawn by a font whose glyph for it is
    /// `advance_px` wide, in a grid of `cell_width_px` cells.
    ///
    /// A symbol is a Nerd Font codepoint, anything in a Private Use Area,
    /// or an ambiguous-width character (one cell by `unicode-width`, two in
    /// East Asian contexts) whose font draws it wider than a cell - the
    /// arrows, circles and stars of Japanese fonts. Procedurally drawn
    /// glyphs (box drawing, blocks, braille, Powerline) are never symbols.
    pub(crate) fn of(ch: char, advance_px: impl FnOnce() -> f32, cell_width_px: f32) -> Self {
        let graphics = is_graphics_element(ch);
        if crate::backend::programmatic_glyphs::is_programmatic_glyph(ch) {
            return Self {
                max_cells: 0,
                graphics,
            };
        }
        let max_cells = if let Some(constraint) = GlyphConstraint::nerd_font(ch) {
            constraint.max_cells
        } else if is_private_use(ch)
            || (ch.width() == Some(1)
                && ch.width_cjk() == Some(2)
                && advance_px() > cell_width_px + 1.0)
        {
            GlyphConstraint::FIT.max_cells
        } else {
            0
        };
        Self {
            max_cells,
            graphics,
        }
    }
}

fn is_private_use(ch: char) -> bool {
    matches!(ch, '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..='\u{FFFFD}' | '\u{100000}'..='\u{10FFFD}')
}

/// Box drawing, block elements, legacy computing symbols and Powerline
/// separators - glyphs that tile with their neighbours.
fn is_graphics_element(ch: char) -> bool {
    matches!(
        ch,
        '\u{2500}'..='\u{259F}'
            | '\u{1FB00}'..='\u{1FBFF}'
            | '\u{1CC00}'..='\u{1CEBF}'
            | '\u{E0B0}'..='\u{E0D7}'
    )
}

/// How many cells a symbol of class `class` gets: two when the next cell is
/// blank (`blank_next`, see [`is_blank`]; false at the end of the row) and
/// it may use two, one otherwise. Ghostty's rule: a symbol right after
/// another symbol (`prev`, the class of the cell before it) stays in one
/// cell, so a run of symbols keeps its alignment; graphics elements
/// (Powerline separators etc.) don't count.
pub(crate) fn symbol_span(class: CellClass, prev: Option<CellClass>, blank_next: bool) -> u32 {
    if class.max_cells < 2 || !blank_next {
        return 1;
    }
    if prev.is_some_and(|prev| prev.is_symbol() && !prev.graphics) {
        return 1;
    }
    2
}

/// A cell that lets the symbol before it spread into it: a space, or an
/// en space (kept by Ghostty as the one other "blank" a program may emit).
pub(crate) fn is_blank(symbol: &str) -> bool {
    matches!(symbol, " " | "\u{2002}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const METRICS: CellMetrics = CellMetrics {
        cell_width: 6.0,
        cell_height: 16.0,
        icon_height_single: 11.0,
    };

    fn approx(a: GlyphRect, b: GlyphRect) {
        let close = |x: f32, y: f32| (x - y).abs() < 1e-3;
        assert!(
            close(a.x, b.x)
                && close(a.y, b.y)
                && close(a.width, b.width)
                && close(a.height, b.height),
            "{a:?} != {b:?}"
        );
    }

    #[test]
    fn the_table_is_sorted_and_covers_nerd_font_icons() {
        let ranges = &nerd_font::RANGES;
        assert!(ranges.windows(2).all(|w| w[0].1 < w[1].0));
        assert!(
            ranges
                .iter()
                .all(|&(a, b, i)| a <= b && (i as usize) < nerd_font::CONSTRAINTS.len())
        );
        // nf-fa-folder, nf-md-ab_testing, nf-cod-account: icons in a 2-cell box.
        for ch in ['\u{F07B}', '\u{F0001}', '\u{EA60}'] {
            let c = GlyphConstraint::nerd_font(ch).expect("patched icon");
            assert_eq!((c.size, c.max_cells), (Size::FitCover1, 2), "{ch:?}");
        }
        // Powerline's hard divider is stretched into exactly one cell.
        let divider = GlyphConstraint::nerd_font('\u{E0B0}').unwrap();
        assert_eq!((divider.size, divider.max_cells), (Size::Stretch, 1));
        assert!(GlyphConstraint::nerd_font('a').is_none());
        assert!(GlyphConstraint::nerd_font('\u{10FFFF}').is_none());
    }

    #[test]
    fn fit_shrinks_an_oversized_glyph_and_keeps_its_aspect_ratio() {
        // A 16 px square icon (Symbols Nerd Font at 16 px) in 6 x 16 cells.
        let icon = GlyphRect {
            x: 0.0,
            y: 1.0,
            width: 16.0,
            height: 16.0,
        };
        let two = GlyphConstraint::FIT.constrain(icon, &METRICS, 2);
        approx(
            two,
            GlyphRect {
                x: 0.0,
                y: 3.0,
                width: 12.0,
                height: 12.0,
            },
        );
        let one = GlyphConstraint::FIT.constrain(icon, &METRICS, 1);
        approx(
            one,
            GlyphRect {
                x: 0.0,
                y: 6.0,
                width: 6.0,
                height: 6.0,
            },
        );
    }

    #[test]
    fn fit_leaves_a_glyph_that_fits_alone() {
        let glyph = GlyphRect {
            x: 1.0,
            y: 2.0,
            width: 4.0,
            height: 10.0,
        };
        assert_eq!(GlyphConstraint::FIT.constrain(glyph, &METRICS, 1), glyph);
        assert_eq!(GlyphConstraint::FIT.constrain(glyph, &METRICS, 2), glyph);
    }

    #[test]
    fn a_patched_icon_grows_to_one_cell_but_not_to_two() {
        let rule = GlyphConstraint::nerd_font('\u{F07B}').unwrap();
        // Small (a Mono font's icon): covers one cell either way, centered
        // in the first cell.
        let small = GlyphRect {
            x: 0.0,
            y: 5.0,
            width: 3.0,
            height: 3.0,
        };
        let one = rule.constrain(small, &METRICS, 1);
        let two = rule.constrain(small, &METRICS, 2);
        approx(one, two);
        approx(
            one,
            GlyphRect {
                x: 0.0,
                y: 5.0,
                width: 6.0,
                height: 6.0,
            },
        );
        // Wide (a non-Mono font's icon, 1.5 cells): keeps its size in two
        // cells, left-aligned; shrinks into one.
        let wide = GlyphRect {
            x: 0.0,
            y: 4.0,
            width: 9.0,
            height: 8.0,
        };
        approx(
            rule.constrain(wide, &METRICS, 2),
            GlyphRect {
                x: 0.0,
                y: 4.0,
                ..wide
            },
        );
        assert!((rule.constrain(wide, &METRICS, 1).width - 6.0).abs() < 1e-3);
    }

    #[test]
    fn spans_follow_ghosttys_rule() {
        let icon = CellClass {
            max_cells: 2,
            graphics: false,
        };
        let one_cell = CellClass {
            max_cells: 1,
            graphics: false,
        };
        let separator = CellClass {
            max_cells: 1,
            graphics: true,
        };
        let text = Some(CellClass::TEXT);
        assert_eq!(symbol_span(icon, None, true), 2);
        assert_eq!(symbol_span(icon, None, false), 1);
        assert_eq!(symbol_span(one_cell, None, true), 1);
        assert_eq!(symbol_span(icon, Some(icon), true), 1, "after a symbol");
        assert_eq!(
            symbol_span(icon, Some(separator), true),
            2,
            "after a separator"
        );
        assert_eq!(symbol_span(icon, text, true), 2);
        assert!(is_blank(" ") && is_blank("\u{2002}") && !is_blank("\u{A0}") && !is_blank(""));
    }

    #[test]
    fn symbols_are_classified_by_codepoint_and_drawn_width() {
        let narrow = || 6.0;
        let wide = || 12.0;
        assert_eq!(CellClass::of('\u{F07B}', narrow, 6.0).max_cells, 2);
        assert_eq!(CellClass::of('\u{E000}', narrow, 6.0).max_cells, 2, "PUA");
        assert_eq!(
            CellClass::of('\u{2630}', narrow, 6.0).max_cells,
            1,
            "patched, one cell"
        );
        assert!(CellClass::of('\u{E0C0}', narrow, 6.0).graphics);
        // Procedural glyphs are drawn by the backend, never constrained.
        assert!(!CellClass::of('\u{E0B0}', wide, 6.0).is_symbol());
        assert!(!CellClass::of('─', wide, 6.0).is_symbol());
        // Ambiguous width: a symbol only where the font draws it wide.
        assert_eq!(CellClass::of('→', wide, 6.0).max_cells, 2);
        assert!(!CellClass::of('→', narrow, 6.0).is_symbol());
        assert!(!CellClass::of('a', wide, 6.0).is_symbol());
        assert!(
            !CellClass::of('あ', wide, 6.0).is_symbol(),
            "already two cells"
        );
    }
}
