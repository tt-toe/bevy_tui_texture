use super::primitives::*;
use tiny_skia::Pixmap;

pub fn render(c: char, width: u32, height: u32) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)?;

    let pattern = (c as u32).checked_sub(0x2800)?;
    if pattern > 0xFF {
        return None;
    }

    let dot_size = stroke_width(height) * 0.6;
    let color = default_color();

    let padding = dot_size * 1.0;
    let usable_width = (width as f32 - 2.0 * padding).max(1.0);
    let usable_height = (height as f32 - 2.0 * padding).max(1.0);

    let cols = 2;
    let rows = 4;
    let cell_w = usable_width / cols as f32;
    let cell_h = usable_height / rows as f32;

    let dot_positions = [
        (0, 0),
        (0, 1),
        (0, 2),
        (1, 0),
        (1, 1),
        (1, 2),
        (0, 3),
        (1, 3),
    ];

    for (bit_index, &(col, row)) in dot_positions.iter().enumerate() {
        if (pattern & (1 << bit_index)) != 0 {
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
        let pixmap = render('\u{2800}', 32, 32).unwrap();
        assert!(pixmap.pixels().iter().all(|p| p.alpha() == 0));
    }

    #[test]
    fn test_braille_full() {
        let pixmap = render('\u{28FF}', 32, 32).unwrap();
        assert!(pixmap.pixels().iter().any(|p| p.alpha() > 0));
    }

    #[test]
    fn test_braille_single_dot() {
        let pixmap = render('\u{2801}', 32, 32).unwrap();
        assert!(pixmap.pixels().iter().any(|p| p.alpha() > 0));
    }
}
