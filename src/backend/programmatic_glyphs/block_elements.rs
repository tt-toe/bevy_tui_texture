use super::primitives::*;

/// Turns on the pixels `on` selects.
fn dither(canvas: &mut GlyphCanvas, on: impl Fn(u32, u32) -> bool) {
    for y in 0..canvas.height() {
        for x in 0..canvas.width() {
            if on(x, y) {
                canvas.set(x, y, 255);
            }
        }
    }
}

pub fn render(c: char, width: u32, height: u32) -> Option<GlyphCanvas> {
    let mut canvas = GlyphCanvas::new(width, height)?;
    let w = width as f32;
    let h = height as f32;
    let color = default_color();

    match c {
        '▀' => draw_rect(&mut canvas, 0.0, 0.0, w, h / 2.0, color),
        '▁' => draw_rect(&mut canvas, 0.0, h - h / 8.0, w, h / 8.0, color),
        '▂' => draw_rect(&mut canvas, 0.0, h - h / 4.0, w, h / 4.0, color),
        '▃' => draw_rect(&mut canvas, 0.0, h - 3.0 * h / 8.0, w, 3.0 * h / 8.0, color),
        '▄' => draw_rect(&mut canvas, 0.0, h / 2.0, w, h / 2.0, color),
        '▅' => draw_rect(&mut canvas, 0.0, h - 5.0 * h / 8.0, w, 5.0 * h / 8.0, color),
        '▆' => draw_rect(&mut canvas, 0.0, h - 3.0 * h / 4.0, w, 3.0 * h / 4.0, color),
        '▇' => draw_rect(&mut canvas, 0.0, h - 7.0 * h / 8.0, w, 7.0 * h / 8.0, color),
        '█' => draw_rect(&mut canvas, 0.0, 0.0, w, h, color),
        '▉' => draw_rect(&mut canvas, 0.0, 0.0, 7.0 * w / 8.0, h, color),
        '▊' => draw_rect(&mut canvas, 0.0, 0.0, 3.0 * w / 4.0, h, color),
        '▋' => draw_rect(&mut canvas, 0.0, 0.0, 5.0 * w / 8.0, h, color),
        '▌' => draw_rect(&mut canvas, 0.0, 0.0, w / 2.0, h, color),
        '▍' => draw_rect(&mut canvas, 0.0, 0.0, 3.0 * w / 8.0, h, color),
        '▎' => draw_rect(&mut canvas, 0.0, 0.0, w / 4.0, h, color),
        '▏' => draw_rect(&mut canvas, 0.0, 0.0, w / 8.0, h, color),

        '▐' => draw_rect(&mut canvas, w / 2.0, 0.0, w / 2.0, h, color),

        // One-pixel dither with a period of two, so it survives any cell
        // size and tiles seamlessly across cells of even width and height.
        '░' => dither(&mut canvas, |x, y| x % 2 == 0 && y % 2 == 0),
        '▒' => dither(&mut canvas, |x, y| (x + y) % 2 == 0),
        '▓' => dither(&mut canvas, |x, y| x % 2 == 0 || y % 2 == 1),

        '▔' => draw_rect(&mut canvas, 0.0, 0.0, w, h / 8.0, color),
        '▕' => draw_rect(&mut canvas, w - w / 8.0, 0.0, w / 8.0, h, color),
        '▖' => draw_rect(&mut canvas, 0.0, h / 2.0, w / 2.0, h / 2.0, color),
        '▗' => draw_rect(&mut canvas, w / 2.0, h / 2.0, w / 2.0, h / 2.0, color),
        '▘' => draw_rect(&mut canvas, 0.0, 0.0, w / 2.0, h / 2.0, color),
        '▙' => {
            draw_rect(&mut canvas, 0.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut canvas, 0.0, h / 2.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut canvas, w / 2.0, h / 2.0, w / 2.0, h / 2.0, color);
        }
        '▚' => {
            draw_rect(&mut canvas, 0.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut canvas, w / 2.0, h / 2.0, w / 2.0, h / 2.0, color);
        }
        '▛' => {
            draw_rect(&mut canvas, 0.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut canvas, w / 2.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut canvas, 0.0, h / 2.0, w / 2.0, h / 2.0, color);
        }
        '▜' => {
            draw_rect(&mut canvas, 0.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut canvas, w / 2.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut canvas, w / 2.0, h / 2.0, w / 2.0, h / 2.0, color);
        }
        '▝' => draw_rect(&mut canvas, w / 2.0, 0.0, w / 2.0, h / 2.0, color),
        '▞' => {
            draw_rect(&mut canvas, w / 2.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut canvas, 0.0, h / 2.0, w / 2.0, h / 2.0, color);
        }
        '▟' => {
            draw_rect(&mut canvas, w / 2.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut canvas, 0.0, h / 2.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut canvas, w / 2.0, h / 2.0, w / 2.0, h / 2.0, color);
        }

        _ => return None,
    }

    Some(canvas)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shades_dither_a_quarter_half_and_three_quarters() {
        for (c, on) in [('░', 32), ('▒', 64), ('▓', 96)] {
            let canvas = render(c, 8, 16).unwrap();
            let lit = canvas.data().iter().filter(|&&a| a == 255).count();
            assert_eq!(lit, on, "{c}");
            assert!(canvas.data().iter().all(|&a| matches!(a, 0 | 255)), "{c}");
        }
    }

    #[test]
    fn shades_tile_across_even_cells() {
        for c in ['░', '▒', '▓'] {
            let canvas = render(c, 8, 16).unwrap();
            let at = |x: usize, y: usize| canvas.data()[y * 8 + x];
            assert!((0..16).all(|y| at(0, y) == at(2, y)), "{c}: columns");
            assert!((0..8).all(|x| at(x, 0) == at(x, 2)), "{c}: rows");
        }
    }
}
