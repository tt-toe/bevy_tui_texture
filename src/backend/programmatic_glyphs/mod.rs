// Programmatic glyph rendering module
//
// This module renders special Unicode glyphs (box-drawing, block elements, braille, powerline)
// programmatically using tiny-skia, then pre-bakes them into the texture atlas at startup.
//
// This approach provides:
// - Zero runtime overhead (glyphs are texture-sampled like fonts)
// - Pixel-perfect rendering at any size
// - No GPU pipeline changes needed

mod block_elements;
mod box_drawing;
mod braille;
mod powerline;
mod primitives;

use tiny_skia::Pixmap;

/// Check if a character should be rendered programmatically
pub fn is_programmatic_glyph(c: char) -> bool {
    matches!(c,
        '\u{2500}'..='\u{257F}' |  // Box Drawing
        '\u{2580}'..='\u{259F}' |  // Block Elements
        '\u{2800}'..='\u{28FF}' |  // Braille Patterns
        '\u{E0B0}'..='\u{E0BF}'    // Powerline Symbols
    )
}

/// Render a programmatic glyph to a bitmap
///
/// Returns a tiny-skia Pixmap containing the rendered glyph, or None if the
/// character is not a programmatic glyph.
///
/// # Arguments
/// * `c` - The Unicode character to render
/// * `width` - Width of the glyph cell in pixels
/// * `height` - Height of the glyph cell in pixels
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
///
/// This returns all 440 glyphs that should be pre-rendered into the atlas:
/// - Box Drawing: 128 glyphs (U+2500–U+257F)
/// - Block Elements: 32 glyphs (U+2580–U+259F)
/// - Braille Patterns: 256 glyphs (U+2800–U+28FF)
/// - Powerline Symbols: 24 glyphs (U+E0B0–U+E0BF)
pub fn all_programmatic_glyphs() -> impl Iterator<Item = char> {
    // Box Drawing (128 glyphs)
    ('\u{2500}'..='\u{257F}')
        // Block Elements (32 glyphs)
        .chain('\u{2580}'..='\u{259F}')
        // Braille Patterns (256 glyphs)
        .chain('\u{2800}'..='\u{28FF}')
        // Powerline Symbols (24 glyphs, but only first 16 in range E0B0-E0BF)
        .chain('\u{E0B0}'..='\u{E0BF}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_programmatic_glyph() {
        // Box drawing
        assert!(is_programmatic_glyph('─'));
        assert!(is_programmatic_glyph('│'));

        // Block elements
        assert!(is_programmatic_glyph('█'));
        assert!(is_programmatic_glyph('▀'));

        // Braille
        assert!(is_programmatic_glyph('⠀'));
        assert!(is_programmatic_glyph('⣿'));

        // Powerline
        assert!(is_programmatic_glyph('\u{E0B0}'));

        // Not programmatic
        assert!(!is_programmatic_glyph('A'));
        assert!(!is_programmatic_glyph('あ'));
    }

    #[test]
    fn test_glyph_count() {
        let count = all_programmatic_glyphs().count();
        assert_eq!(count, 128 + 32 + 256 + 16); // 432 glyphs in the defined ranges
    }
}
