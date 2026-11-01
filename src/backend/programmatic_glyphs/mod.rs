mod block_elements;
mod box_drawing;
mod braille;
mod powerline;
mod primitives;

use tiny_skia::Pixmap;

/// Check if a character should be rendered programmatically
pub fn is_programmatic_glyph(c: char) -> bool {
    matches!(c,
        '\u{2500}'..='\u{257F}' |
        '\u{2580}'..='\u{259F}' |
        '\u{2800}'..='\u{28FF}' |
        '\u{E0B0}'..='\u{E0BF}'
    )
}

/// Render a programmatic glyph to a bitmap
pub fn render_programmatic_glyph(c: char, width: u32, height: u32) -> Option<Pixmap> {
    match c {
        '\u{2500}'..='\u{257F}' => box_drawing::render(c, width, height),
        '\u{2580}'..='\u{259F}' => block_elements::render(c, width, height),
        '\u{2800}'..='\u{28FF}' => braille::render(c, width, height),
        '\u{E0B0}'..='\u{E0BF}' => powerline::render(c, width, height),
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

    #[test]
    fn test_glyph_count() {
        let count = all_programmatic_glyphs().count();
        assert_eq!(count, 128 + 32 + 256 + 16);
    }
}
