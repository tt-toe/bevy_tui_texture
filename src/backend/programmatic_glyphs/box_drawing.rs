// Box Drawing Characters (U+2500–U+257F)
//
// Implements programmatic rendering for 128 box-drawing glyphs.
// Based on Rio Terminal's implementation in batch.rs

use super::primitives::*;
use tiny_skia::Pixmap;

pub fn render(c: char, width: u32, height: u32) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)?;
    let stroke = stroke_width(height);
    let heavy_stroke = stroke * 2.0;
    let center_x = width as f32 / 2.0;
    let center_y = height as f32 / 2.0;
    let w = width as f32;
    let h = height as f32;
    let color = default_color();

    match c {
        // ═══ Basic Lines ═══
        '─' => draw_horizontal_line(&mut pixmap, center_y, stroke, color), // U+2500
        '━' => draw_horizontal_line(&mut pixmap, center_y, heavy_stroke, color), // U+2501 Heavy
        '│' => draw_vertical_line(&mut pixmap, center_x, stroke, color),   // U+2502
        '┃' => draw_vertical_line(&mut pixmap, center_x, heavy_stroke, color), // U+2503 Heavy

        // ═══ Dashed Lines ═══
        '┄' => {
            // U+2504 Light triple dash horizontal
            let dash_count = 3;
            let gap = stroke;
            let dash_width = (w - (dash_count - 1) as f32 * gap) / dash_count as f32;
            for i in 0..dash_count {
                let x = i as f32 * (dash_width + gap);
                draw_rect(
                    &mut pixmap,
                    x,
                    center_y - stroke / 2.0,
                    dash_width,
                    stroke,
                    color,
                );
            }
        }
        '┅' => {
            // U+2505 Heavy triple dash horizontal
            let dash_count = 3;
            let gap = stroke;
            let dash_width = (w - (dash_count - 1) as f32 * gap) / dash_count as f32;
            for i in 0..dash_count {
                let x = i as f32 * (dash_width + gap);
                draw_rect(
                    &mut pixmap,
                    x,
                    center_y - heavy_stroke / 2.0,
                    dash_width,
                    heavy_stroke,
                    color,
                );
            }
        }
        '┆' => {
            // U+2506 Light triple dash vertical
            let dash_count = 3;
            let gap = stroke;
            let dash_height = (h - (dash_count - 1) as f32 * gap) / dash_count as f32;
            for i in 0..dash_count {
                let y = i as f32 * (dash_height + gap);
                draw_rect(
                    &mut pixmap,
                    center_x - stroke / 2.0,
                    y,
                    stroke,
                    dash_height,
                    color,
                );
            }
        }
        '┇' => {
            // U+2507 Heavy triple dash vertical
            let dash_count = 3;
            let gap = stroke;
            let dash_height = (h - (dash_count - 1) as f32 * gap) / dash_count as f32;
            for i in 0..dash_count {
                let y = i as f32 * (dash_height + gap);
                draw_rect(
                    &mut pixmap,
                    center_x - heavy_stroke / 2.0,
                    y,
                    heavy_stroke,
                    dash_height,
                    color,
                );
            }
        }
        '┈' => {
            // U+2508 Light quadruple dash horizontal
            let dash_count = 4;
            let gap = stroke;
            let dash_width = (w - (dash_count - 1) as f32 * gap) / dash_count as f32;
            for i in 0..dash_count {
                let x = i as f32 * (dash_width + gap);
                draw_rect(
                    &mut pixmap,
                    x,
                    center_y - stroke / 2.0,
                    dash_width,
                    stroke,
                    color,
                );
            }
        }
        '┉' => {
            // U+2509 Heavy quadruple dash horizontal
            let dash_count = 4;
            let gap = stroke;
            let dash_width = (w - (dash_count - 1) as f32 * gap) / dash_count as f32;
            for i in 0..dash_count {
                let x = i as f32 * (dash_width + gap);
                draw_rect(
                    &mut pixmap,
                    x,
                    center_y - heavy_stroke / 2.0,
                    dash_width,
                    heavy_stroke,
                    color,
                );
            }
        }
        '┊' => {
            // U+250A Light quadruple dash vertical
            let dash_count = 4;
            let gap = stroke;
            let dash_height = (h - (dash_count - 1) as f32 * gap) / dash_count as f32;
            for i in 0..dash_count {
                let y = i as f32 * (dash_height + gap);
                draw_rect(
                    &mut pixmap,
                    center_x - stroke / 2.0,
                    y,
                    stroke,
                    dash_height,
                    color,
                );
            }
        }
        '┋' => {
            // U+250B Heavy quadruple dash vertical
            let dash_count = 4;
            let gap = stroke;
            let dash_height = (h - (dash_count - 1) as f32 * gap) / dash_count as f32;
            for i in 0..dash_count {
                let y = i as f32 * (dash_height + gap);
                draw_rect(
                    &mut pixmap,
                    center_x - heavy_stroke / 2.0,
                    y,
                    heavy_stroke,
                    dash_height,
                    color,
                );
            }
        }

        // ═══ Corners (Light) ═══
        '┌' => {
            // U+250C Down and right
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                center_y - stroke / 2.0,
                w / 2.0 + stroke / 2.0,
                stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                center_y - stroke / 2.0,
                stroke,
                h / 2.0 + stroke / 2.0,
                color,
            );
        }
        '┐' => {
            // U+2510 Down and left
            draw_rect(
                &mut pixmap,
                0.0,
                center_y - stroke / 2.0,
                center_x + stroke / 2.0,
                stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                center_y - stroke / 2.0,
                stroke,
                h / 2.0 + stroke / 2.0,
                color,
            );
        }
        '└' => {
            // U+2514 Up and right
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                center_y - stroke / 2.0,
                w / 2.0 + stroke / 2.0,
                stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                0.0,
                stroke,
                center_y + stroke / 2.0,
                color,
            );
        }
        '┘' => {
            // U+2518 Up and left
            draw_rect(
                &mut pixmap,
                0.0,
                center_y - stroke / 2.0,
                center_x + stroke / 2.0,
                stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                0.0,
                stroke,
                center_y + stroke / 2.0,
                color,
            );
        }

        // ═══ Corners (Heavy) ═══
        '┏' => {
            // U+250F Heavy down and right
            draw_rect(
                &mut pixmap,
                center_x - heavy_stroke / 2.0,
                center_y - heavy_stroke / 2.0,
                w / 2.0 + heavy_stroke / 2.0,
                heavy_stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - heavy_stroke / 2.0,
                center_y - heavy_stroke / 2.0,
                heavy_stroke,
                h / 2.0 + heavy_stroke / 2.0,
                color,
            );
        }
        '┓' => {
            // U+2513 Heavy down and left
            draw_rect(
                &mut pixmap,
                0.0,
                center_y - heavy_stroke / 2.0,
                center_x + heavy_stroke / 2.0,
                heavy_stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - heavy_stroke / 2.0,
                center_y - heavy_stroke / 2.0,
                heavy_stroke,
                h / 2.0 + heavy_stroke / 2.0,
                color,
            );
        }
        '┗' => {
            // U+2517 Heavy up and right
            draw_rect(
                &mut pixmap,
                center_x - heavy_stroke / 2.0,
                center_y - heavy_stroke / 2.0,
                w / 2.0 + heavy_stroke / 2.0,
                heavy_stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - heavy_stroke / 2.0,
                0.0,
                heavy_stroke,
                center_y + heavy_stroke / 2.0,
                color,
            );
        }
        '┛' => {
            // U+251B Heavy up and left
            draw_rect(
                &mut pixmap,
                0.0,
                center_y - heavy_stroke / 2.0,
                center_x + heavy_stroke / 2.0,
                heavy_stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - heavy_stroke / 2.0,
                0.0,
                heavy_stroke,
                center_y + heavy_stroke / 2.0,
                color,
            );
        }

        // ═══ T-junctions (Light) ═══
        '├' => {
            // U+251C Vertical and right
            draw_vertical_line(&mut pixmap, center_x, stroke, color);
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                center_y - stroke / 2.0,
                w / 2.0 + stroke / 2.0,
                stroke,
                color,
            );
        }
        '┤' => {
            // U+2524 Vertical and left
            draw_vertical_line(&mut pixmap, center_x, stroke, color);
            draw_rect(
                &mut pixmap,
                0.0,
                center_y - stroke / 2.0,
                center_x + stroke / 2.0,
                stroke,
                color,
            );
        }
        '┬' => {
            // U+252C Horizontal and down
            draw_horizontal_line(&mut pixmap, center_y, stroke, color);
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                center_y - stroke / 2.0,
                stroke,
                h / 2.0 + stroke / 2.0,
                color,
            );
        }
        '┴' => {
            // U+2534 Horizontal and up
            draw_horizontal_line(&mut pixmap, center_y, stroke, color);
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                0.0,
                stroke,
                center_y + stroke / 2.0,
                color,
            );
        }

        // ═══ Cross ═══
        '┼' => {
            // U+253C Vertical and horizontal
            draw_horizontal_line(&mut pixmap, center_y, stroke, color);
            draw_vertical_line(&mut pixmap, center_x, stroke, color);
        }

        // ═══ Double Lines ═══
        '═' => {
            // U+2550 Double horizontal - use thinner strokes for each line
            let thin_stroke = (stroke * 0.6).max(1.0);
            let gap = stroke * 0.8;
            draw_horizontal_line(&mut pixmap, center_y - gap, thin_stroke, color);
            draw_horizontal_line(&mut pixmap, center_y + gap, thin_stroke, color);
        }
        '║' => {
            // U+2551 Double vertical - use thinner strokes for each line
            let thin_stroke = (stroke * 0.6).max(1.0);
            let gap = stroke * 0.8;
            draw_vertical_line(&mut pixmap, center_x - gap, thin_stroke, color);
            draw_vertical_line(&mut pixmap, center_x + gap, thin_stroke, color);
        }

        // ═══ Double Line Corners ═══
        '╔' => {
            // U+2554 Double down and right
            let thin_stroke = (stroke * 0.6).max(1.0);
            let gap = stroke * 0.8;
            // Outer horizontal line (top)
            draw_rect(
                &mut pixmap,
                center_x - gap,
                center_y - gap - thin_stroke / 2.0,
                w / 2.0 + gap,
                thin_stroke,
                color,
            );
            // Outer vertical line (left)
            draw_rect(
                &mut pixmap,
                center_x - gap - thin_stroke / 2.0,
                center_y - gap,
                thin_stroke,
                h / 2.0 + gap,
                color,
            );
            // Inner horizontal line (bottom) - extend to corner
            draw_rect(
                &mut pixmap,
                center_x + gap - thin_stroke / 2.0,
                center_y + gap - thin_stroke / 2.0,
                w / 2.0 - gap + thin_stroke / 2.0,
                thin_stroke,
                color,
            );
            // Inner vertical line (right) - extend to corner
            draw_rect(
                &mut pixmap,
                center_x + gap - thin_stroke / 2.0,
                center_y + gap - thin_stroke / 2.0,
                thin_stroke,
                h / 2.0 - gap + thin_stroke / 2.0,
                color,
            );
        }
        '╗' => {
            // U+2557 Double down and left
            let thin_stroke = (stroke * 0.6).max(1.0);
            let gap = stroke * 0.8;
            // Outer horizontal line (top)
            draw_rect(
                &mut pixmap,
                0.0,
                center_y - gap - thin_stroke / 2.0,
                center_x + gap,
                thin_stroke,
                color,
            );
            // Outer vertical line (right)
            draw_rect(
                &mut pixmap,
                center_x + gap - thin_stroke / 2.0,
                center_y - gap,
                thin_stroke,
                h / 2.0 + gap,
                color,
            );
            // Inner horizontal line (bottom) - extend to corner
            draw_rect(
                &mut pixmap,
                0.0,
                center_y + gap - thin_stroke / 2.0,
                center_x - gap + thin_stroke / 2.0,
                thin_stroke,
                color,
            );
            // Inner vertical line (left) - extend to corner
            draw_rect(
                &mut pixmap,
                center_x - gap - thin_stroke / 2.0,
                center_y + gap - thin_stroke / 2.0,
                thin_stroke,
                h / 2.0 - gap + thin_stroke / 2.0,
                color,
            );
        }
        '╚' => {
            // U+255A Double up and right
            let thin_stroke = (stroke * 0.6).max(1.0);
            let gap = stroke * 0.8;
            // Outer horizontal line (bottom)
            draw_rect(
                &mut pixmap,
                center_x - gap,
                center_y + gap - thin_stroke / 2.0,
                w / 2.0 + gap,
                thin_stroke,
                color,
            );
            // Outer vertical line (left)
            draw_rect(
                &mut pixmap,
                center_x - gap - thin_stroke / 2.0,
                0.0,
                thin_stroke,
                center_y + gap,
                color,
            );
            // Inner horizontal line (top) - extend to corner
            draw_rect(
                &mut pixmap,
                center_x + gap - thin_stroke / 2.0,
                center_y - gap - thin_stroke / 2.0,
                w / 2.0 - gap + thin_stroke / 2.0,
                thin_stroke,
                color,
            );
            // Inner vertical line (right) - extend to corner
            draw_rect(
                &mut pixmap,
                center_x + gap - thin_stroke / 2.0,
                0.0,
                thin_stroke,
                center_y - gap + thin_stroke / 2.0,
                color,
            );
        }
        '╝' => {
            // U+255D Double up and left
            let thin_stroke = (stroke * 0.6).max(1.0);
            let gap = stroke * 0.8;
            // Outer horizontal line (bottom)
            draw_rect(
                &mut pixmap,
                0.0,
                center_y + gap - thin_stroke / 2.0,
                center_x + gap,
                thin_stroke,
                color,
            );
            // Outer vertical line (right)
            draw_rect(
                &mut pixmap,
                center_x + gap - thin_stroke / 2.0,
                0.0,
                thin_stroke,
                center_y + gap,
                color,
            );
            // Inner horizontal line (top) - extend to corner
            draw_rect(
                &mut pixmap,
                0.0,
                center_y - gap - thin_stroke / 2.0,
                center_x - gap + thin_stroke / 2.0,
                thin_stroke,
                color,
            );
            // Inner vertical line (left) - extend to corner
            draw_rect(
                &mut pixmap,
                center_x - gap - thin_stroke / 2.0,
                0.0,
                thin_stroke,
                center_y - gap + thin_stroke / 2.0,
                color,
            );
        }

        // ═══ Arc Corners ═══
        '╭' => {
            // U+256D Arc down and right - larger radius for more pronounced curve
            let radius = w / 2.5;
            draw_rect(
                &mut pixmap,
                center_x + radius,
                center_y - stroke / 2.0,
                w / 2.0 - radius,
                stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                center_y + radius,
                stroke,
                h / 2.0 - radius,
                color,
            );
            draw_arc(
                &mut pixmap,
                center_x + radius,
                center_y + radius,
                radius,
                180.0,
                270.0,
                stroke,
                color,
            );
        }
        '╮' => {
            // U+256E Arc down and left - larger radius for more pronounced curve
            let radius = w / 2.5;
            draw_rect(
                &mut pixmap,
                0.0,
                center_y - stroke / 2.0,
                center_x - radius,
                stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                center_y + radius,
                stroke,
                h / 2.0 - radius,
                color,
            );
            draw_arc(
                &mut pixmap,
                center_x - radius,
                center_y + radius,
                radius,
                270.0,
                360.0,
                stroke,
                color,
            );
        }
        '╯' => {
            // U+256F Arc up and left - larger radius for more pronounced curve
            let radius = w / 2.5;
            draw_rect(
                &mut pixmap,
                0.0,
                center_y - stroke / 2.0,
                center_x - radius,
                stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                0.0,
                stroke,
                center_y - radius,
                color,
            );
            draw_arc(
                &mut pixmap,
                center_x - radius,
                center_y - radius,
                radius,
                0.0,
                90.0,
                stroke,
                color,
            );
        }
        '╰' => {
            // U+2570 Arc up and right - larger radius for more pronounced curve
            let radius = w / 2.5;
            draw_rect(
                &mut pixmap,
                center_x + radius,
                center_y - stroke / 2.0,
                w / 2.0 - radius,
                stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                0.0,
                stroke,
                center_y - radius,
                color,
            );
            draw_arc(
                &mut pixmap,
                center_x + radius,
                center_y - radius,
                radius,
                90.0,
                180.0,
                stroke,
                color,
            );
        }

        // ═══ Diagonal Lines ═══
        '╱' => {
            // U+2571 Diagonal rising (bottom-left to top-right)
            draw_line(&mut pixmap, 0.0, h, w, 0.0, stroke, color);
        }
        '╲' => {
            // U+2572 Diagonal falling (top-left to bottom-right)
            draw_line(&mut pixmap, 0.0, 0.0, w, h, stroke, color);
        }
        '╳' => {
            // U+2573 Diagonal cross
            draw_line(&mut pixmap, 0.0, h, w, 0.0, stroke, color);
            draw_line(&mut pixmap, 0.0, 0.0, w, h, stroke, color);
        }

        // TODO: Implement remaining glyphs (U+250C-U+257F)
        // - More double line combinations
        // - Heavy line variants
        // - Mixed heavy/light combinations
        // See Rio's batch.rs for complete implementation
        _ => return None, // Unsupported glyph (will be added incrementally)
    }

    Some(pixmap)
}
