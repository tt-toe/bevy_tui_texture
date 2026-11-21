use super::box_lines::{LineMetrics, dashes, draw_lines, lines_of};
use super::primitives::*;

pub fn render(c: char, width: u32, height: u32) -> Option<GlyphCanvas> {
    let mut canvas = GlyphCanvas::new(width, height)?;
    let m = LineMetrics::new(width, height);
    if let Some(lines) = lines_of(c) {
        draw_lines(&mut canvas, &m, lines);
        return Some(canvas);
    }

    let w = width as f32;
    let h = height as f32;
    let light = m.light as f32;
    let color = default_color();
    let cx = m.v_light_left as f32 + light / 2.0;
    let cy = m.h_light_top as f32 + light / 2.0;
    let top = m.h_light_top as f32;
    let left = m.v_light_left as f32;

    match c {
        '┄' => dashes(&mut canvas, &m, 3, false, false),
        '┅' => dashes(&mut canvas, &m, 3, true, false),
        '┆' => dashes(&mut canvas, &m, 3, false, true),
        '┇' => dashes(&mut canvas, &m, 3, true, true),
        '┈' => dashes(&mut canvas, &m, 4, false, false),
        '┉' => dashes(&mut canvas, &m, 4, true, false),
        '┊' => dashes(&mut canvas, &m, 4, false, true),
        '┋' => dashes(&mut canvas, &m, 4, true, true),
        '╌' => dashes(&mut canvas, &m, 2, false, false),
        '╍' => dashes(&mut canvas, &m, 2, true, false),
        '╎' => dashes(&mut canvas, &m, 2, false, true),
        '╏' => dashes(&mut canvas, &m, 2, true, true),

        '╭' | '╮' | '╯' | '╰' => {
            let radius = w.min(h) / 2.5;
            let (east, south) = match c {
                '╭' => (true, true),
                '╮' => (false, true),
                '╯' => (false, false),
                _ => (true, false),
            };
            let (hx, hw) = if east {
                (cx + radius, w - (cx + radius))
            } else {
                (0.0, cx - radius)
            };
            let (vy, vh) = if south {
                (cy + radius, h - (cy + radius))
            } else {
                (0.0, cy - radius)
            };
            draw_rect(&mut canvas, hx, top, hw, light, color);
            draw_rect(&mut canvas, left, vy, light, vh, color);
            let acx = if east { cx + radius } else { cx - radius };
            let acy = if south { cy + radius } else { cy - radius };
            let (a0, a1) = match c {
                '╭' => (180.0, 270.0),
                '╮' => (270.0, 360.0),
                '╯' => (0.0, 90.0),
                _ => (90.0, 180.0),
            };
            draw_arc(&mut canvas, acx, acy, radius, a0, a1, light, color);
        }

        '╱' => draw_line(&mut canvas, 0.0, h, w, 0.0, light, color),
        '╲' => draw_line(&mut canvas, 0.0, 0.0, w, h, light, color),
        '╳' => {
            draw_line(&mut canvas, 0.0, h, w, 0.0, light, color);
            draw_line(&mut canvas, 0.0, 0.0, w, h, light, color);
        }

        _ => return None,
    }

    Some(canvas)
}
