use super::primitives::*;
use tiny_skia::{Pixmap, PremultipliedColorU8};

/// Turns on the pixels `on` selects.
fn dither(pixmap: &mut Pixmap, on: impl Fn(u32, u32) -> bool) {
    let Some(white) = PremultipliedColorU8::from_rgba(255, 255, 255, 255) else {
        return;
    };
    let width = pixmap.width().max(1);
    for (i, pixel) in pixmap.pixels_mut().iter_mut().enumerate() {
        if on(i as u32 % width, i as u32 / width) {
            *pixel = white;
        }
    }
}

pub fn render(c: char, width: u32, height: u32) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)?;
    let w = width as f32;
    let h = height as f32;
    let color = default_color();

    match c {
        '▀' => draw_rect(&mut pixmap, 0.0, 0.0, w, h / 2.0, color),
        '▁' => draw_rect(&mut pixmap, 0.0, h - h / 8.0, w, h / 8.0, color),
        '▂' => draw_rect(&mut pixmap, 0.0, h - h / 4.0, w, h / 4.0, color),
        '▃' => draw_rect(&mut pixmap, 0.0, h - 3.0 * h / 8.0, w, 3.0 * h / 8.0, color),
        '▄' => draw_rect(&mut pixmap, 0.0, h / 2.0, w, h / 2.0, color),
        '▅' => draw_rect(&mut pixmap, 0.0, h - 5.0 * h / 8.0, w, 5.0 * h / 8.0, color),
        '▆' => draw_rect(&mut pixmap, 0.0, h - 3.0 * h / 4.0, w, 3.0 * h / 4.0, color),
        '▇' => draw_rect(&mut pixmap, 0.0, h - 7.0 * h / 8.0, w, 7.0 * h / 8.0, color),
        '█' => draw_rect(&mut pixmap, 0.0, 0.0, w, h, color),
        '▉' => draw_rect(&mut pixmap, 0.0, 0.0, 7.0 * w / 8.0, h, color),
        '▊' => draw_rect(&mut pixmap, 0.0, 0.0, 3.0 * w / 4.0, h, color),
        '▋' => draw_rect(&mut pixmap, 0.0, 0.0, 5.0 * w / 8.0, h, color),
        '▌' => draw_rect(&mut pixmap, 0.0, 0.0, w / 2.0, h, color),
        '▍' => draw_rect(&mut pixmap, 0.0, 0.0, 3.0 * w / 8.0, h, color),
        '▎' => draw_rect(&mut pixmap, 0.0, 0.0, w / 4.0, h, color),
        '▏' => draw_rect(&mut pixmap, 0.0, 0.0, w / 8.0, h, color),

        '▐' => draw_rect(&mut pixmap, w / 2.0, 0.0, w / 2.0, h, color),

        // One-pixel dither with a period of two, so it survives any cell
        // size and tiles seamlessly across cells of even width and height.
        '░' => dither(&mut pixmap, |x, y| x % 2 == 0 && y % 2 == 0),
        '▒' => dither(&mut pixmap, |x, y| (x + y) % 2 == 0),
        '▓' => dither(&mut pixmap, |x, y| x % 2 == 0 || y % 2 == 1),

        '▔' => draw_rect(&mut pixmap, 0.0, 0.0, w, h / 8.0, color),
        '▕' => draw_rect(&mut pixmap, w - w / 8.0, 0.0, w / 8.0, h, color),
        '▖' => draw_rect(&mut pixmap, 0.0, h / 2.0, w / 2.0, h / 2.0, color),
        '▗' => draw_rect(&mut pixmap, w / 2.0, h / 2.0, w / 2.0, h / 2.0, color),
        '▘' => draw_rect(&mut pixmap, 0.0, 0.0, w / 2.0, h / 2.0, color),
        '▙' => {
            draw_rect(&mut pixmap, 0.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut pixmap, 0.0, h / 2.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut pixmap, w / 2.0, h / 2.0, w / 2.0, h / 2.0, color);
        }
        '▚' => {
            draw_rect(&mut pixmap, 0.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut pixmap, w / 2.0, h / 2.0, w / 2.0, h / 2.0, color);
        }
        '▛' => {
            draw_rect(&mut pixmap, 0.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut pixmap, w / 2.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut pixmap, 0.0, h / 2.0, w / 2.0, h / 2.0, color);
        }
        '▜' => {
            draw_rect(&mut pixmap, 0.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut pixmap, w / 2.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut pixmap, w / 2.0, h / 2.0, w / 2.0, h / 2.0, color);
        }
        '▝' => draw_rect(&mut pixmap, w / 2.0, 0.0, w / 2.0, h / 2.0, color),
        '▞' => {
            draw_rect(&mut pixmap, w / 2.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut pixmap, 0.0, h / 2.0, w / 2.0, h / 2.0, color);
        }
        '▟' => {
            draw_rect(&mut pixmap, w / 2.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut pixmap, 0.0, h / 2.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut pixmap, w / 2.0, h / 2.0, w / 2.0, h / 2.0, color);
        }

        _ => return None,
    }

    Some(pixmap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shades_dither_a_quarter_half_and_three_quarters() {
        for (c, on) in [('░', 32), ('▒', 64), ('▓', 96)] {
            let pixmap = render(c, 8, 16).unwrap();
            let lit = pixmap.pixels().iter().filter(|p| p.alpha() == 255).count();
            assert_eq!(lit, on, "{c}");
            assert!(
                pixmap.pixels().iter().all(|p| matches!(p.alpha(), 0 | 255)),
                "{c}"
            );
        }
    }

    #[test]
    fn shades_tile_across_even_cells() {
        for c in ['░', '▒', '▓'] {
            let pixmap = render(c, 8, 16).unwrap();
            let at = |x: usize, y: usize| pixmap.pixels()[y * 8 + x].alpha();
            assert!((0..16).all(|y| at(0, y) == at(2, y)), "{c}: columns");
            assert!((0..8).all(|x| at(x, 0) == at(x, 2)), "{c}: rows");
        }
    }
}
