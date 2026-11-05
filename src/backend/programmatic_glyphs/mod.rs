mod block_elements;
mod box_drawing;
pub(crate) mod box_lines;
mod braille;
pub(crate) mod fallback_symbols;
mod geometric;
mod mosaic;
mod powerline;
mod primitives;

use tiny_skia::Pixmap;

/// Check if a character should be rendered programmatically
#[inline]
pub fn is_programmatic_glyph(c: char) -> bool {
    // Called per glyph: one compare settles all text below the first range.
    if (c as u32) < 0x2500 {
        return false;
    }
    matches!(c,
        '\u{2500}'..='\u{257F}' |
        '\u{2580}'..='\u{259F}' |
        '\u{2800}'..='\u{28FF}' |
        '\u{E0B0}'..='\u{E0BF}' |
        '\u{1FB00}'..='\u{1FB3B}' |
        '\u{1CD00}'..='\u{1CDE5}' |
        '\u{1CEA0}' | '\u{1CEA3}' | '\u{1CEA8}' | '\u{1CEAB}' |
        '\u{1FB82}' | '\u{1FB85}' | '\u{1FBE6}' | '\u{1FBE7}' |
        '\u{1FB3C}'..='\u{1FB67}' |
        '\u{25E2}'..='\u{25E5}' | '\u{25F8}'..='\u{25FA}' | '\u{25FF}'
    )
}

/// Render a programmatic glyph to a bitmap
pub fn render_programmatic_glyph(c: char, width: u32, height: u32) -> Option<Pixmap> {
    match c {
        '\u{2500}'..='\u{257F}' => box_drawing::render(c, width, height),
        '\u{2580}'..='\u{259F}' => block_elements::render(c, width, height),
        '\u{2800}'..='\u{28FF}' => braille::render(c, width, height),
        '\u{E0B0}'..='\u{E0BF}' => powerline::render(c, width, height),
        '\u{1FB00}'..='\u{1FB3B}'
        | '\u{1CD00}'..='\u{1CDE5}'
        | '\u{1CEA0}'
        | '\u{1CEA3}'
        | '\u{1CEA8}'
        | '\u{1CEAB}'
        | '\u{1FB82}'
        | '\u{1FB85}'
        | '\u{1FBE6}'
        | '\u{1FBE7}'
        | '\u{1FB3C}'..='\u{1FB67}' => mosaic::render(c, width, height),
        '\u{25E2}'..='\u{25E5}' | '\u{25F8}'..='\u{25FA}' | '\u{25FF}' => {
            geometric::render(c, width, height)
        }
        _ => None,
    }
}

/// Get an iterator over all programmatic glyphs for eager pre-population
pub fn all_programmatic_glyphs() -> impl Iterator<Item = char> {
    ('\u{2500}'..='\u{257F}')
        .chain('\u{2580}'..='\u{259F}')
        .chain('\u{2800}'..='\u{28FF}')
        .chain('\u{E0B0}'..='\u{E0BF}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_programmatic_glyph() {
        assert!(is_programmatic_glyph('─'));
        assert!(is_programmatic_glyph('│'));

        assert!(is_programmatic_glyph('█'));
        assert!(is_programmatic_glyph('▀'));

        assert!(is_programmatic_glyph('⠀'));
        assert!(is_programmatic_glyph('⣿'));

        assert!(is_programmatic_glyph('\u{E0B0}'));

        assert!(!is_programmatic_glyph('A'));
        assert!(!is_programmatic_glyph('あ'));
    }

    const SIZES: [(u32, u32); 6] = [(7, 14), (8, 16), (10, 20), (11, 23), (16, 33), (9, 19)];

    fn covered(p: &Pixmap, x: u32, y: u32) -> bool {
        p.pixel(x, y).unwrap().alpha() > 0
    }

    fn column(p: &Pixmap, x: u32) -> Vec<bool> {
        (0..p.height()).map(|y| covered(p, x, y)).collect()
    }

    fn row(p: &Pixmap, y: u32) -> Vec<bool> {
        (0..p.width()).map(|x| covered(p, x, y)).collect()
    }

    #[test]
    fn every_box_drawing_char_renders() {
        for (w, h) in SIZES {
            for c in '\u{2500}'..='\u{257F}' {
                assert!(
                    render_programmatic_glyph(c, w, h).is_some(),
                    "{c:?} {w}x{h}"
                );
            }
        }
    }

    #[test]
    fn box_lines_join_across_cells() {
        use box_lines::{Weight, lines_of};
        let mut count = 0;
        for (w, h) in SIZES {
            let draw = |c| render_programmatic_glyph(c, w, h).unwrap();
            let (hl, hh, hd) = (draw('─'), draw('━'), draw('═'));
            let (vl, vh, vd) = (draw('│'), draw('┃'), draw('║'));
            let horiz = |wt: Weight, x: u32| match wt {
                Weight::None => vec![false; h as usize],
                Weight::Light => column(&hl, x),
                Weight::Heavy => column(&hh, x),
                Weight::Double => column(&hd, x),
            };
            let vert = |wt: Weight, y: u32| match wt {
                Weight::None => vec![false; w as usize],
                Weight::Light => row(&vl, y),
                Weight::Heavy => row(&vh, y),
                Weight::Double => row(&vd, y),
            };
            for c in '\u{2500}'..='\u{257F}' {
                let Some(lines) = lines_of(c) else { continue };
                if (w, h) == SIZES[0] {
                    count += 1;
                }
                let p = draw(c);
                assert_eq!(
                    column(&p, w - 1),
                    horiz(lines.right, w - 1),
                    "{c:?} right {w}x{h}"
                );
                assert_eq!(column(&p, 0), horiz(lines.left, 0), "{c:?} left {w}x{h}");
                assert_eq!(row(&p, 0), vert(lines.up, 0), "{c:?} up {w}x{h}");
                assert_eq!(
                    row(&p, h - 1),
                    vert(lines.down, h - 1),
                    "{c:?} down {w}x{h}"
                );
            }
        }
        assert_eq!(count, 109);
    }

    #[test]
    fn rounded_corners_join_with_lines() {
        for (w, h) in SIZES {
            let draw = |c| render_programmatic_glyph(c, w, h).unwrap();
            let (hl, vl) = (draw('─'), draw('│'));
            let arcs = [
                ('╭', true, true),
                ('╮', false, true),
                ('╯', false, false),
                ('╰', true, false),
            ];
            for (c, east, south) in arcs {
                let p = draw(c);
                let hx = if east { w - 1 } else { 0 };
                let vy = if south { h - 1 } else { 0 };
                assert_eq!(column(&p, hx), column(&hl, hx), "{c:?} horizontal {w}x{h}");
                assert_eq!(row(&p, vy), row(&vl, vy), "{c:?} vertical {w}x{h}");
            }
        }
    }

    #[test]
    fn dashes_leave_half_gaps_at_both_ends() {
        for (w, h) in SIZES {
            for (c, count) in [('┄', 3), ('┈', 4), ('╌', 2), ('┅', 3), ('╍', 2)] {
                if w / (2 * count) < 2 {
                    continue;
                }
                let p = render_programmatic_glyph(c, w, h).unwrap();
                let mid = row(&p, h / 2);
                assert!(!mid[0] && !mid[w as usize - 1], "{c:?} {w}x{h}");
                assert!(mid.iter().any(|&b| b), "{c:?} {w}x{h}");
            }
        }
    }

    #[test]
    fn table_matches_ratatui_line_sets() {
        use ratatui::symbols::line::*;
        let sets = [
            NORMAL,
            ROUNDED,
            DOUBLE,
            THICK,
            LIGHT_DOUBLE_DASHED,
            HEAVY_DOUBLE_DASHED,
            LIGHT_TRIPLE_DASHED,
            HEAVY_TRIPLE_DASHED,
            LIGHT_QUADRUPLE_DASHED,
            HEAVY_QUADRUPLE_DASHED,
        ];
        for set in sets {
            let fields = [
                set.vertical,
                set.horizontal,
                set.top_right,
                set.top_left,
                set.bottom_right,
                set.bottom_left,
                set.vertical_left,
                set.vertical_right,
                set.horizontal_down,
                set.horizontal_up,
                set.cross,
            ];
            for symbol in fields {
                for c in symbol.chars() {
                    assert!(render_programmatic_glyph(c, 10, 20).is_some(), "{c:?}");
                }
            }
        }
    }

    #[test]
    fn ratatui_symbols_are_all_drawable() {
        use ratatui::symbols::{bar, block, border, line, scrollbar, shade};

        fn line_set(s: line::Set<'_>) -> Vec<&str> {
            vec![
                s.vertical,
                s.horizontal,
                s.top_right,
                s.top_left,
                s.bottom_right,
                s.bottom_left,
                s.vertical_left,
                s.vertical_right,
                s.horizontal_down,
                s.horizontal_up,
                s.cross,
            ]
        }
        fn border_set(s: border::Set<'_>) -> Vec<&str> {
            vec![
                s.top_left,
                s.top_right,
                s.bottom_left,
                s.bottom_right,
                s.vertical_left,
                s.vertical_right,
                s.horizontal_top,
                s.horizontal_bottom,
            ]
        }
        fn bar_set(s: bar::Set<'_>) -> Vec<&str> {
            vec![
                s.full,
                s.seven_eighths,
                s.three_quarters,
                s.five_eighths,
                s.half,
                s.three_eighths,
                s.one_quarter,
                s.one_eighth,
                s.empty,
            ]
        }
        fn block_set(s: block::Set<'_>) -> Vec<&str> {
            vec![
                s.full,
                s.seven_eighths,
                s.three_quarters,
                s.five_eighths,
                s.half,
                s.three_eighths,
                s.one_quarter,
                s.one_eighth,
                s.empty,
            ]
        }
        fn scrollbar_set(s: scrollbar::Set<'_>) -> Vec<&str> {
            vec![s.track, s.thumb, s.begin, s.end]
        }

        let mut symbols: Vec<&str> = Vec::new();
        for set in [
            line::NORMAL,
            line::ROUNDED,
            line::DOUBLE,
            line::THICK,
            line::LIGHT_DOUBLE_DASHED,
            line::HEAVY_DOUBLE_DASHED,
            line::LIGHT_TRIPLE_DASHED,
            line::HEAVY_TRIPLE_DASHED,
            line::LIGHT_QUADRUPLE_DASHED,
            line::HEAVY_QUADRUPLE_DASHED,
        ] {
            symbols.extend(line_set(set));
        }
        for set in [
            border::PLAIN,
            border::ROUNDED,
            border::DOUBLE,
            border::THICK,
            border::QUADRANT_OUTSIDE,
            border::QUADRANT_INSIDE,
            border::ONE_EIGHTH_WIDE,
            border::ONE_EIGHTH_TALL,
            border::PROPORTIONAL_WIDE,
            border::PROPORTIONAL_TALL,
            border::FULL,
            border::EMPTY,
            border::LIGHT_DOUBLE_DASHED,
            border::HEAVY_DOUBLE_DASHED,
            border::LIGHT_TRIPLE_DASHED,
            border::HEAVY_TRIPLE_DASHED,
            border::LIGHT_QUADRUPLE_DASHED,
            border::HEAVY_QUADRUPLE_DASHED,
        ] {
            symbols.extend(border_set(set));
        }
        symbols.extend([
            border::QUADRANT_TOP_LEFT,
            border::QUADRANT_TOP_RIGHT,
            border::QUADRANT_BOTTOM_LEFT,
            border::QUADRANT_BOTTOM_RIGHT,
            border::QUADRANT_TOP_HALF,
            border::QUADRANT_BOTTOM_HALF,
            border::QUADRANT_LEFT_HALF,
            border::QUADRANT_RIGHT_HALF,
            border::QUADRANT_TOP_LEFT_BOTTOM_LEFT_BOTTOM_RIGHT,
            border::QUADRANT_TOP_LEFT_TOP_RIGHT_BOTTOM_LEFT,
            border::QUADRANT_TOP_LEFT_TOP_RIGHT_BOTTOM_RIGHT,
            border::QUADRANT_TOP_RIGHT_BOTTOM_LEFT_BOTTOM_RIGHT,
            border::QUADRANT_TOP_LEFT_BOTTOM_RIGHT,
            border::QUADRANT_TOP_RIGHT_BOTTOM_LEFT,
            border::QUADRANT_BLOCK,
            border::ONE_EIGHTH_TOP_EIGHT,
            border::ONE_EIGHTH_BOTTOM_EIGHT,
            border::ONE_EIGHTH_LEFT_EIGHT,
            border::ONE_EIGHTH_RIGHT_EIGHT,
            shade::EMPTY,
            shade::LIGHT,
            shade::MEDIUM,
            shade::DARK,
            shade::FULL,
            ratatui::symbols::marker::DOT,
            "×",
        ]);
        for set in [bar::THREE_LEVELS, bar::NINE_LEVELS] {
            symbols.extend(bar_set(set));
        }
        for set in [block::THREE_LEVELS, block::NINE_LEVELS] {
            symbols.extend(block_set(set));
        }
        for set in [
            scrollbar::DOUBLE_VERTICAL,
            scrollbar::DOUBLE_HORIZONTAL,
            scrollbar::VERTICAL,
            scrollbar::HORIZONTAL,
        ] {
            symbols.extend(scrollbar_set(set));
        }

        let mut chars: Vec<char> = symbols.iter().flat_map(|s| s.chars()).collect();
        chars.extend(['\u{2580}', '\u{2584}', '\u{2588}']);
        chars.extend('\u{2500}'..='\u{257F}');
        chars.extend(ratatui::symbols::braille::BRAILLE);
        chars.extend(ratatui::symbols::pixel::QUADRANTS);
        chars.extend(ratatui::symbols::pixel::SEXTANTS);
        chars.extend(ratatui::symbols::pixel::OCTANTS);
        for c in chars {
            assert!(
                c.is_ascii()
                    || c.is_whitespace()
                    || render_programmatic_glyph(c, 10, 20).is_some()
                    || fallback_symbols::render(c, 10, 20).is_some(),
                "{c:?} (U+{:04X}) cannot be drawn",
                c as u32
            );
        }
    }

    #[test]
    fn test_glyph_count() {
        let count = all_programmatic_glyphs().count();
        assert_eq!(count, 128 + 32 + 256 + 16);
    }
}
