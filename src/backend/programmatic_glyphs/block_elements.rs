// Block Element Characters (U+2580–U+259F)
//
// Implements programmatic rendering for 32 block element glyphs.

use super::primitives::*;
use tiny_skia::Pixmap;

pub fn render(c: char, width: u32, height: u32) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)?;
    let w = width as f32;
    let h = height as f32;
    let color = default_color();

    match c {
        // ═══ Half Blocks ═══
        '▀' => draw_rect(&mut pixmap, 0.0, 0.0, w, h / 2.0, color), // U+2580 Upper half
        '▁' => draw_rect(&mut pixmap, 0.0, h - h / 8.0, w, h / 8.0, color), // U+2581 Lower 1/8
        '▂' => draw_rect(&mut pixmap, 0.0, h - h / 4.0, w, h / 4.0, color), // U+2582 Lower 1/4
        '▃' => draw_rect(&mut pixmap, 0.0, h - 3.0 * h / 8.0, w, 3.0 * h / 8.0, color), // U+2583 Lower 3/8
        '▄' => draw_rect(&mut pixmap, 0.0, h / 2.0, w, h / 2.0, color), // U+2584 Lower half
        '▅' => draw_rect(&mut pixmap, 0.0, h - 5.0 * h / 8.0, w, 5.0 * h / 8.0, color), // U+2585 Lower 5/8
        '▆' => draw_rect(&mut pixmap, 0.0, h - 3.0 * h / 4.0, w, 3.0 * h / 4.0, color), // U+2586 Lower 3/4
        '▇' => draw_rect(&mut pixmap, 0.0, h - 7.0 * h / 8.0, w, 7.0 * h / 8.0, color), // U+2587 Lower 7/8
        '█' => draw_rect(&mut pixmap, 0.0, 0.0, w, h, color), // U+2588 Full block
        '▉' => draw_rect(&mut pixmap, 0.0, 0.0, 7.0 * w / 8.0, h, color), // U+2589 Left 7/8
        '▊' => draw_rect(&mut pixmap, 0.0, 0.0, 3.0 * w / 4.0, h, color), // U+258A Left 3/4
        '▋' => draw_rect(&mut pixmap, 0.0, 0.0, 5.0 * w / 8.0, h, color), // U+258B Left 5/8
        '▌' => draw_rect(&mut pixmap, 0.0, 0.0, w / 2.0, h, color), // U+258C Left half
        '▍' => draw_rect(&mut pixmap, 0.0, 0.0, 3.0 * w / 8.0, h, color), // U+258D Left 3/8
        '▎' => draw_rect(&mut pixmap, 0.0, 0.0, w / 4.0, h, color), // U+258E Left 1/4
        '▏' => draw_rect(&mut pixmap, 0.0, 0.0, w / 8.0, h, color), // U+258F Left 1/8

        // ═══ Right Blocks ═══
        '▐' => draw_rect(&mut pixmap, w / 2.0, 0.0, w / 2.0, h, color), // U+2590 Right half

        // ═══ Shade Patterns ═══
        '░' => {
            // U+2591 Light shade (25% filled)
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
            // U+2592 Medium shade (50% filled)
            let dot_size = stroke_width(height) * 0.5;
            let cols = 4;
            let rows = 8;
            let cell_w = w / cols as f32;
            let cell_h = h / rows as f32;

            // Draw base pattern (like light shade)
            for row in 0..rows {
                for col in 0..cols {
                    if (row % 2 == 0 && col % 2 == 0) || (row % 2 == 1 && col % 2 == 1) {
                        let x = col as f32 * cell_w + (cell_w - dot_size) / 2.0;
                        let y = row as f32 * cell_h + (cell_h - dot_size) / 2.0;
                        draw_rect(&mut pixmap, x, y, dot_size, dot_size, color);
                    }
                }
            }

            // Add secondary pattern (offset)
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
            // U+2593 Dark shade (75% filled) - for now render as solid
            // TODO: Implement proper dark shade pattern with pixel manipulation
            draw_rect(&mut pixmap, 0.0, 0.0, w, h, color);
        }

        // ═══ Quadrants ═══
        '▔' => draw_rect(&mut pixmap, 0.0, 0.0, w, h / 8.0, color), // U+2594 Upper 1/8
        '▕' => draw_rect(&mut pixmap, w - w / 8.0, 0.0, w / 8.0, h, color), // U+2595 Right 1/8
        '▖' => draw_rect(&mut pixmap, 0.0, h / 2.0, w / 2.0, h / 2.0, color), // U+2596 Lower left quadrant
        '▗' => draw_rect(&mut pixmap, w / 2.0, h / 2.0, w / 2.0, h / 2.0, color), // U+2597 Lower right quadrant
        '▘' => draw_rect(&mut pixmap, 0.0, 0.0, w / 2.0, h / 2.0, color), // U+2598 Upper left quadrant
        '▙' => {
            // U+2599 Upper left and lower left and lower right
            draw_rect(&mut pixmap, 0.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut pixmap, 0.0, h / 2.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut pixmap, w / 2.0, h / 2.0, w / 2.0, h / 2.0, color);
        }
        '▚' => {
            // U+259A Upper left and lower right
            draw_rect(&mut pixmap, 0.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut pixmap, w / 2.0, h / 2.0, w / 2.0, h / 2.0, color);
        }
        '▛' => {
            // U+259B Upper left and upper right and lower left
            draw_rect(&mut pixmap, 0.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut pixmap, w / 2.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut pixmap, 0.0, h / 2.0, w / 2.0, h / 2.0, color);
        }
        '▜' => {
            // U+259C Upper left and upper right and lower right
            draw_rect(&mut pixmap, 0.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut pixmap, w / 2.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut pixmap, w / 2.0, h / 2.0, w / 2.0, h / 2.0, color);
        }
        '▝' => draw_rect(&mut pixmap, w / 2.0, 0.0, w / 2.0, h / 2.0, color), // U+259D Upper right quadrant
        '▞' => {
            // U+259E Upper right and lower left
            draw_rect(&mut pixmap, w / 2.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut pixmap, 0.0, h / 2.0, w / 2.0, h / 2.0, color);
        }
        '▟' => {
            // U+259F Upper right and lower left and lower right
            draw_rect(&mut pixmap, w / 2.0, 0.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut pixmap, 0.0, h / 2.0, w / 2.0, h / 2.0, color);
            draw_rect(&mut pixmap, w / 2.0, h / 2.0, w / 2.0, h / 2.0, color);
        }

        _ => return None,
    }

    Some(pixmap)
}
