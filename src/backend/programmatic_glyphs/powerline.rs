// Powerline Symbol Characters (U+E0B0–U+E0BF)
//
// Implements programmatic rendering for 16 powerline symbols.
// These are commonly used in shell prompts and status bars.

use super::primitives::*;
use tiny_skia::Pixmap;

pub fn render(c: char, width: u32, height: u32) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)?;
    let w = width as f32;
    let h = height as f32;
    let color = default_color();
    let stroke = stroke_width(height) * 0.5;

    match c {
        // ═══ Solid Triangles ═══
        '\u{E0B0}' => {
            // Right-pointing solid triangle
            draw_triangle(&mut pixmap, 0.0, 0.0, 0.0, h, w, h / 2.0, color);
        }
        '\u{E0B1}' => {
            // Right-pointing hollow triangle (line)
            draw_line(&mut pixmap, 0.0, 0.0, w, h / 2.0, stroke, color);
            draw_line(&mut pixmap, 0.0, h, w, h / 2.0, stroke, color);
        }
        '\u{E0B2}' => {
            // Left-pointing solid triangle
            draw_triangle(&mut pixmap, w, 0.0, w, h, 0.0, h / 2.0, color);
        }
        '\u{E0B3}' => {
            // Left-pointing hollow triangle (line)
            draw_line(&mut pixmap, w, 0.0, 0.0, h / 2.0, stroke, color);
            draw_line(&mut pixmap, w, h, 0.0, h / 2.0, stroke, color);
        }

        // ═══ Curved Variants ═══
        '\u{E0B4}' => {
            // Right-pointing curved (solid)
            // Draw a filled bezier curve approximated by polygon
            let segments = 60;
            let mut points = Vec::with_capacity(segments + 2);

            // Right edge (straight)
            points.push((w, 0.0));
            points.push((w, h));

            // Curved left edge (half oval)
            for i in (0..=segments).rev() {
                let t = i as f32 / segments as f32;
                let normalized_y = 2.0 * t - 1.0;
                let x_pos = w - (w * (1.0 - normalized_y * normalized_y).sqrt());
                let y_pos = t * h;
                points.push((x_pos, y_pos));
            }

            draw_polygon(&mut pixmap, &points, color);
        }
        '\u{E0B5}' => {
            // Right-pointing curved (hollow)
            // Draw curved lines at top and bottom
            let segments = 30;
            for i in 0..segments {
                let t1 = i as f32 / segments as f32;
                let t2 = (i + 1) as f32 / segments as f32;

                let norm_y1 = 2.0 * t1 - 1.0;
                let norm_y2 = 2.0 * t2 - 1.0;

                let x1 = w - (w * (1.0 - norm_y1 * norm_y1).sqrt());
                let y1 = t1 * h;
                let x2 = w - (w * (1.0 - norm_y2 * norm_y2).sqrt());
                let y2 = t2 * h;

                draw_line(&mut pixmap, x1, y1, x2, y2, stroke, color);
            }

            // Horizontal lines at top and bottom
            draw_line(&mut pixmap, 0.0, 0.0, w, 0.0, stroke, color);
            draw_line(&mut pixmap, 0.0, h, w, h, stroke, color);
        }
        '\u{E0B6}' => {
            // Left-pointing curved (solid)
            let segments = 60;
            let mut points = Vec::with_capacity(segments + 2);

            // Left edge (straight)
            points.push((0.0, 0.0));
            points.push((0.0, h));

            // Curved right edge
            for i in 0..=segments {
                let t = i as f32 / segments as f32;
                let normalized_y = 2.0 * t - 1.0;
                let x_pos = w * (1.0 - normalized_y * normalized_y).sqrt();
                let y_pos = t * h;
                points.push((x_pos, y_pos));
            }

            draw_polygon(&mut pixmap, &points, color);
        }
        '\u{E0B7}' => {
            // Left-pointing curved (hollow)
            let segments = 30;
            for i in 0..segments {
                let t1 = i as f32 / segments as f32;
                let t2 = (i + 1) as f32 / segments as f32;

                let norm_y1 = 2.0 * t1 - 1.0;
                let norm_y2 = 2.0 * t2 - 1.0;

                let x1 = w * (1.0 - norm_y1 * norm_y1).sqrt();
                let y1 = t1 * h;
                let x2 = w * (1.0 - norm_y2 * norm_y2).sqrt();
                let y2 = t2 * h;

                draw_line(&mut pixmap, x1, y1, x2, y2, stroke, color);
            }

            // Horizontal lines
            draw_line(&mut pixmap, 0.0, 0.0, w, 0.0, stroke, color);
            draw_line(&mut pixmap, 0.0, h, w, h, stroke, color);
        }

        // ═══ Additional Powerline Symbols ═══
        '\u{E0B8}' => {
            // Lower left triangle
            draw_triangle(&mut pixmap, 0.0, h, w, h, 0.0, 0.0, color);
        }
        '\u{E0B9}' => {
            // Backslash separator
            draw_line(&mut pixmap, w, 0.0, 0.0, h, stroke, color);
        }
        '\u{E0BA}' => {
            // Lower right triangle
            draw_triangle(&mut pixmap, 0.0, h, w, h, w, 0.0, color);
        }
        '\u{E0BB}' => {
            // Forward slash separator
            draw_line(&mut pixmap, 0.0, 0.0, w, h, stroke, color);
        }
        '\u{E0BC}' => {
            // Upper left triangle
            draw_triangle(&mut pixmap, 0.0, 0.0, w, 0.0, 0.0, h, color);
        }
        '\u{E0BD}' => {
            // Forward slash separator (variant)
            draw_line(&mut pixmap, 0.0, 0.0, w, h, stroke, color);
        }
        '\u{E0BE}' => {
            // Upper right triangle
            draw_triangle(&mut pixmap, 0.0, 0.0, w, 0.0, w, h, color);
        }
        '\u{E0BF}' => {
            // Backslash separator (variant)
            draw_line(&mut pixmap, w, 0.0, 0.0, h, stroke, color);
        }

        _ => return None,
    }

    Some(pixmap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_powerline_right_triangle() {
        let pixmap = render('\u{E0B0}', 32, 32).unwrap();
        assert!(pixmap.pixels().iter().any(|p| p.alpha() > 0));
    }

    #[test]
    fn test_powerline_left_triangle() {
        let pixmap = render('\u{E0B2}', 32, 32).unwrap();
        assert!(pixmap.pixels().iter().any(|p| p.alpha() > 0));
    }
}
