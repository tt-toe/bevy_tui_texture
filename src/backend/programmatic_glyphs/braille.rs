// Braille Pattern Characters (U+2800–U+28FF)
//
// Implements programmatic rendering for 256 braille patterns.
// Each pattern is encoded as a bit pattern where each bit represents a dot.

use super::primitives::*;
use tiny_skia::Pixmap;

pub fn render(c: char, width: u32, height: u32) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)?;

    // Braille base is U+2800, pattern is the offset
    let pattern = (c as u32).checked_sub(0x2800)?;
    if pattern > 0xFF {
        return None; // Out of range
    }

    // Braille uses a 2×4 grid of dots:
    // ╭───┬───╮
    // │ 0 │ 3 │  Bit 0 = dot 1 (top-left)
    // ├───┼───┤  Bit 1 = dot 2 (middle-left)
    // │ 1 │ 4 │  Bit 2 = dot 3 (lower-left)
    // ├───┼───┤  Bit 3 = dot 4 (top-right)
    // │ 2 │ 5 │  Bit 4 = dot 5 (middle-right)
    // ├───┼───┤  Bit 5 = dot 6 (lower-right)
    // │ 6 │ 7 │  Bit 6 = dot 7 (bottom-left)
    // ╰───┴───╯  Bit 7 = dot 8 (bottom-right)

    let dot_size = stroke_width(height) * 0.6;
    let color = default_color();

    // Add padding to prevent dots from being clipped at edges
    let padding = dot_size * 1.0;
    let usable_width = (width as f32 - 2.0 * padding).max(1.0);
    let usable_height = (height as f32 - 2.0 * padding).max(1.0);

    // Grid dimensions
    let cols = 2;
    let rows = 4;
    let cell_w = usable_width / cols as f32;
    let cell_h = usable_height / rows as f32;

    // Dot bit mapping: [bit_index] = (column, row)
    let dot_positions = [
        (0, 0), // Bit 0: dot 1
        (0, 1), // Bit 1: dot 2
        (0, 2), // Bit 2: dot 3
        (1, 0), // Bit 3: dot 4
        (1, 1), // Bit 4: dot 5
        (1, 2), // Bit 5: dot 6
        (0, 3), // Bit 6: dot 7
        (1, 3), // Bit 7: dot 8
    ];

    // Draw dots based on bit pattern
    for (bit_index, &(col, row)) in dot_positions.iter().enumerate() {
        if (pattern & (1 << bit_index)) != 0 {
            // Calculate position within the usable area, then add padding
            let x = padding + col as f32 * cell_w + (cell_w - dot_size) / 2.0;
            let y = padding + row as f32 * cell_h + (cell_h - dot_size) / 2.0;
            draw_circle(
                &mut pixmap,
                x + dot_size / 2.0,
                y + dot_size / 2.0,
                dot_size / 2.0,
                color,
            );
        }
    }

    Some(pixmap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_braille_blank() {
        // U+2800 is blank (no dots)
        let pixmap = render('\u{2800}', 32, 32).unwrap();
        // Should have no pixels
        assert!(pixmap.pixels().iter().all(|p| p.alpha() == 0));
    }

    #[test]
    fn test_braille_full() {
        // U+28FF is full (all 8 dots)
        let pixmap = render('\u{28FF}', 32, 32).unwrap();
        // Should have pixels
        assert!(pixmap.pixels().iter().any(|p| p.alpha() > 0));
    }

    #[test]
    fn test_braille_single_dot() {
        // U+2801 is dot 1 only (top-left)
        let pixmap = render('\u{2801}', 32, 32).unwrap();
        assert!(pixmap.pixels().iter().any(|p| p.alpha() > 0));
    }
}
