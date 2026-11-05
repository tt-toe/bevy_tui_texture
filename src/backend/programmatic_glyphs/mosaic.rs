use super::primitives::*;
use tiny_skia::Pixmap;

/// Ghostty's `SmoothMosaic` patterns for U+1FB3C..=U+1FB67: four rows of three grid points, `#` marking a polygon corner.
const WEDGES: [&str; 44] = [
    "...|...|#..|##.",
    "...|...|#\\.|###",
    "...|#..|#\\.|##.",
    "...|#..|##.|###",
    "#..|#..|##.|##.",
    "/##|###|###|###",
    "./#|###|###|###",
    ".##|.##|###|###",
    "..#|.##|###|###",
    ".##|.##|.##|###",
    "...|./#|###|###",
    "...|...|..#|.##",
    "...|...|./#|###",
    "...|..#|./#|.##",
    "...|..#|.##|###",
    "..#|..#|.##|.##",
    "##\\|###|###|###",
    "#\\.|###|###|###",
    "##.|##.|###|###",
    "#..|##.|###|###",
    "##.|##.|##.|###",
    "...|#\\.|###|###",
    "###|###|###|\\##",
    "###|###|###|.\\#",
    "###|###|.##|.##",
    "###|###|.##|..#",
    "###|.##|.##|.##",
    "##.|#..|...|...",
    "###|#/.|...|...",
    "##.|#/.|#..|...",
    "###|##.|#..|...",
    "##.|##.|#..|#..",
    "###|###|#/.|...",
    "###|###|###|##/",
    "###|###|###|#/.",
    "###|###|##.|##.",
    "###|###|##.|#..",
    "###|##.|##.|##.",
    ".##|..#|...|...",
    "###|.\\#|...|...",
    ".##|.\\#|..#|...",
    "###|.##|..#|...",
    ".##|.##|..#|..#",
    "###|###|.\\#|...",
];

/// Sextant (2x3) and octant (2x4) cells, by inverting ratatui's own tables, and the wedges.
pub fn render(c: char, width: u32, height: u32) -> Option<Pixmap> {
    use ratatui::symbols::pixel::{OCTANTS, SEXTANTS};
    if ('\u{1FB3C}'..='\u{1FB67}').contains(&c) {
        render_wedge(c, width, height)
    } else if let Some(mask) = SEXTANTS.iter().position(|&s| s == c) {
        render_mask(mask, 3, width, height)
    } else if let Some(mask) = OCTANTS.iter().position(|&s| s == c) {
        render_mask(mask, 4, width, height)
    } else {
        None
    }
}

/// Polygon through the patterns' corner points, skipping points on a straight edge.
fn render_wedge(c: char, width: u32, height: u32) -> Option<Pixmap> {
    let index = (c as usize).checked_sub(0x1FB3C)?;
    let pattern: Vec<bool> = WEDGES
        .get(index)?
        .bytes()
        .filter(|&b| b != b'|')
        .map(|b| b == b'#')
        .collect();
    let on = |i: usize| pattern.get(i).copied().unwrap_or(false);
    let edge = |i: usize, a: usize, b: usize| on(i) && (!on(a) || !on(b));
    let (w, h) = (width as f32, height as f32);
    let point = |i: usize| ((i % 3) as f32 * w / 2.0, (i / 3) as f32 * h / 3.0);
    let candidates = [
        (0, on(0)),
        (3, edge(3, 0, 6)),
        (6, edge(6, 3, 9)),
        (9, on(9)),
        (10, edge(10, 9, 11)),
        (11, on(11)),
        (8, edge(8, 11, 5)),
        (5, edge(5, 8, 2)),
        (2, on(2)),
        (1, edge(1, 0, 2)),
    ];
    let points: Vec<(f32, f32)> = candidates
        .iter()
        .filter(|&&(_, used)| used)
        .map(|&(i, _)| point(i))
        .collect();
    let mut pixmap = Pixmap::new(width, height)?;
    draw_polygon(&mut pixmap, &points, default_color());
    Some(pixmap)
}

// Bit i is column i % 2, row i / 2, least significant bit at the top left.
fn render_mask(mask: usize, rows: usize, width: u32, height: u32) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)?;
    let (w, h) = (width as f32, height as f32);
    let color = default_color();
    for row in 0..rows {
        for col in 0..2 {
            if mask >> (row * 2 + col) & 1 == 1 {
                let (x, y) = (col as f32 * w / 2.0, row as f32 * h / rows as f32);
                draw_rect(&mut pixmap, x, y, w / 2.0, h / rows as f32, color);
            }
        }
    }
    Some(pixmap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::programmatic_glyphs::render_programmatic_glyph;
    use ratatui::symbols::pixel::{OCTANTS, QUADRANTS, SEXTANTS};

    #[test]
    fn every_pixel_marker_char_renders() {
        for c in QUADRANTS.into_iter().chain(SEXTANTS).chain(OCTANTS) {
            if c != ' ' {
                assert!(render_programmatic_glyph(c, 10, 20).is_some(), "{c:?}");
            }
        }
    }

    #[test]
    fn mosaic_ranges_are_all_drawn() {
        for c in ('\u{1FB00}'..='\u{1FB3B}')
            .chain('\u{1CD00}'..='\u{1CDE5}')
            .chain(['\u{1CEA0}', '\u{1CEA3}', '\u{1CEA8}', '\u{1CEAB}'])
            .chain(['\u{1FB82}', '\u{1FB85}', '\u{1FBE6}', '\u{1FBE7}'])
        {
            assert!(render(c, 9, 17).is_some(), "{c:?}");
        }
    }

    #[test]
    fn mosaic_matches_block_elements() {
        for (w, h) in [(10, 20), (7, 14), (11, 23)] {
            let left = render_mask(0b01_0101, 3, w, h).unwrap();
            let half = render_programmatic_glyph('▌', w, h).unwrap();
            assert_eq!(left.data(), half.data(), "sextant left column {w}x{h}");

            let bottom = render_mask(0b1100_0000, 4, w, h).unwrap();
            let lower = render_programmatic_glyph('▂', w, h).unwrap();
            assert_eq!(bottom.data(), lower.data(), "octant bottom row {w}x{h}");
        }
        assert_eq!(OCTANTS[0b1100_0000], '▂');
    }

    #[test]
    fn every_wedge_renders() {
        for c in '\u{1FB3C}'..='\u{1FB67}' {
            let pixmap = render(c, 10, 20).unwrap();
            assert!(pixmap.pixels().iter().any(|p| p.alpha() > 0), "{c:?}");
        }
    }

    #[test]
    fn wedge_1fb3c_shape() {
        let pixmap = render('\u{1FB3C}', 10, 20).unwrap();
        assert!(pixmap.pixel(0, 19).unwrap().alpha() > 0);
        assert_eq!(pixmap.pixel(9, 0).unwrap().alpha(), 0);
        assert_eq!(pixmap.pixel(9, 19).unwrap().alpha(), 0);
        assert_eq!(pixmap.pixel(0, 0).unwrap().alpha(), 0);
    }

    #[test]
    fn pixel_bits_are_row_major() {
        let pixmap = render(OCTANTS[3], 10, 20).unwrap();
        for y in 0..20 {
            for x in 0..10 {
                let filled = pixmap.pixel(x, y).unwrap().alpha() > 0;
                assert_eq!(filled, y < 5, "({x},{y})");
            }
        }
    }
}
