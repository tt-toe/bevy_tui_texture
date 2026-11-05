use super::primitives::*;
use tiny_skia::Pixmap;

pub fn render(c: char, width: u32, height: u32) -> Option<Pixmap> {
    let pattern = (c as u32).checked_sub(0x2800).filter(|&p| p <= 0xFF)?;
    let mut pixmap = Pixmap::new(width, height)?;
    let color = default_color();

    let (xs, ys, dot) = dot_grid(width as i32, height as i32);
    // Bit order of the Braille block: dots 1-3 down the left column, 4-6
    // down the right, then 7 and 8 on the bottom row.
    let dots = [
        (0, 0),
        (0, 1),
        (0, 2),
        (1, 0),
        (1, 1),
        (1, 2),
        (0, 3),
        (1, 3),
    ];
    for (bit, &(col, row)) in dots.iter().enumerate() {
        if pattern & (1 << bit) != 0 {
            let (x, y) = (xs[col], ys[row]);
            draw_rect(
                &mut pixmap,
                x as f32,
                y as f32,
                dot as f32,
                dot as f32,
                color,
            );
        }
    }
    Some(pixmap)
}

/// Square dots on whole pixels with equal gaps, distributing leftover pixels
/// the way Ghostty does, so dots never merge or vary in size.
fn dot_grid(width: i32, height: i32) -> ([i32; 2], [i32; 4], i32) {
    let mut dot = (width / 4).min(height / 8);
    let (mut x_gap, mut y_gap) = (width / 4, height / 8);
    let (mut x_margin, mut y_margin) = (x_gap / 2, y_gap / 2);
    let mut x_left = width - 2 * x_margin - x_gap - 2 * dot;
    let mut y_left = height - 2 * y_margin - 3 * y_gap - 4 * dot;

    if x_left >= 2 && y_left >= 4 && dot == 0 {
        dot += 1;
        x_left -= 2;
        y_left -= 4;
    }
    if x_left >= 2 && x_margin == 0 {
        x_margin = 1;
        x_left -= 2;
    }
    if y_left >= 2 && y_margin == 0 {
        y_margin = 1;
        y_left -= 2;
    }
    if x_left >= 1 {
        x_gap += 1;
        x_left -= 1;
    }
    if y_left >= 3 {
        y_gap += 1;
        y_left -= 3;
    }
    if x_left >= 2 {
        x_margin += 1;
        x_left -= 2;
    }
    if y_left >= 2 {
        y_margin += 1;
        y_left -= 2;
    }
    if x_left >= 2 && y_left >= 4 {
        dot += 1;
    }

    let xs = [x_margin, x_margin + dot + x_gap];
    let step = dot + y_gap;
    let ys = [
        y_margin,
        y_margin + step,
        y_margin + 2 * step,
        y_margin + 3 * step,
    ];
    (xs, ys, dot)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coverage(pixmap: &Pixmap) -> Vec<bool> {
        pixmap.pixels().iter().map(|p| p.alpha() > 0).collect()
    }

    #[test]
    fn dots_are_equal_squares_with_gaps_between() {
        for (w, h) in [(7, 14), (8, 16), (10, 20), (11, 23), (16, 33)] {
            let (xs, ys, dot) = dot_grid(w, h);
            assert!(dot >= 1, "{w}x{h}");
            assert!(xs[0] + dot < xs[1], "{w}x{h}: columns touch");
            assert!(
                ys.windows(2).all(|p| p[0] + dot < p[1]),
                "{w}x{h}: rows touch"
            );
            assert!(
                xs[1] + dot <= w && ys[3] + dot <= h,
                "{w}x{h}: outside the cell"
            );
        }
    }

    #[test]
    fn every_dot_is_drawn_separately() {
        let (w, h) = (8, 16);
        let pixmap = render('\u{28FF}', w, h).unwrap();
        let on = coverage(&pixmap);
        let (xs, _, dot) = dot_grid(w as i32, h as i32);
        let filled = on.iter().filter(|&&o| o).count() as i32;
        assert_eq!(filled, 8 * dot * dot);
        let gap_x = (xs[0] + dot) as usize;
        assert!((0..h as usize).all(|y| !on[y * w as usize + gap_x]));
    }

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
