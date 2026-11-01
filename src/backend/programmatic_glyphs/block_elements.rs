use super::primitives::*;
use tiny_skia::Pixmap;

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

        '░' => {
            let dot_size = stroke_width(height) * 0.5;
            let cols = 4;
            let rows = 8;
            let cell_w = w / cols as f32;
            let cell_h = h / rows as f32;

            for row in 0..rows {
                for col in 0..cols {
                    if (row % 2 == 0 && col % 2 == 0) || (row % 2 == 1 && col % 2 == 1) {
                        let x = col as f32 * cell_w + (cell_w - dot_size) / 2.0;
                        let y = row as f32 * cell_h + (cell_h - dot_size) / 2.0;
                        draw_rect(&mut pixmap, x, y, dot_size, dot_size, color);
                    }
                }
            }
        }
        '▒' => {
            let dot_size = stroke_width(height) * 0.5;
            let cols = 4;
            let rows = 8;
            let cell_w = w / cols as f32;
            let cell_h = h / rows as f32;

            for row in 0..rows {
                for col in 0..cols {
                    if (row % 2 == 0 && col % 2 == 0) || (row % 2 == 1 && col % 2 == 1) {
                        let x = col as f32 * cell_w + (cell_w - dot_size) / 2.0;
                        let y = row as f32 * cell_h + (cell_h - dot_size) / 2.0;
                        draw_rect(&mut pixmap, x, y, dot_size, dot_size, color);
                    }
                }
            }

            let small_dot = dot_size * 0.6;
            for row in 0..rows {
                for col in 0..cols {
                    if (row % 2 == 0 && col % 2 == 1) || (row % 2 == 1 && col % 2 == 0) {
                        let x = col as f32 * cell_w + (cell_w - small_dot) / 2.0;
                        let y = row as f32 * cell_h + (cell_h - small_dot) / 2.0;
                        draw_rect(&mut pixmap, x, y, small_dot, small_dot, color);
                    }
                }
            }
        }
        '▓' => {
            draw_rect(&mut pixmap, 0.0, 0.0, w, h, color);
        }

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
