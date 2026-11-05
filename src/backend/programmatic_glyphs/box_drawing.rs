use super::box_lines::{LineMetrics, dash_horizontal, dash_vertical, draw_lines, lines_of};
use super::primitives::*;
use tiny_skia::Pixmap;

pub fn render(c: char, width: u32, height: u32) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)?;
    let m = LineMetrics::new(width, height);
    if let Some(lines) = lines_of(c) {
        draw_lines(&mut pixmap, &m, lines);
        return Some(pixmap);
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
        '┄' => dash_horizontal(&mut pixmap, &m, 3, false),
        '┅' => dash_horizontal(&mut pixmap, &m, 3, true),
        '┆' => dash_vertical(&mut pixmap, &m, 3, false),
        '┇' => dash_vertical(&mut pixmap, &m, 3, true),
        '┈' => dash_horizontal(&mut pixmap, &m, 4, false),
        '┉' => dash_horizontal(&mut pixmap, &m, 4, true),
        '┊' => dash_vertical(&mut pixmap, &m, 4, false),
        '┋' => dash_vertical(&mut pixmap, &m, 4, true),
        '╌' => dash_horizontal(&mut pixmap, &m, 2, false),
        '╍' => dash_horizontal(&mut pixmap, &m, 2, true),
        '╎' => dash_vertical(&mut pixmap, &m, 2, false),
        '╏' => dash_vertical(&mut pixmap, &m, 2, true),

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
            draw_rect(&mut pixmap, hx, top, hw, light, color);
            draw_rect(&mut pixmap, left, vy, light, vh, color);
            let acx = if east { cx + radius } else { cx - radius };
            let acy = if south { cy + radius } else { cy - radius };
            let (a0, a1) = match c {
                '╭' => (180.0, 270.0),
                '╮' => (270.0, 360.0),
                '╯' => (0.0, 90.0),
                _ => (90.0, 180.0),
            };
            draw_arc(&mut pixmap, acx, acy, radius, a0, a1, light, color);
        }

        '╱' => draw_line(&mut pixmap, 0.0, h, w, 0.0, light, color),
        '╲' => draw_line(&mut pixmap, 0.0, 0.0, w, h, light, color),
        '╳' => {
            draw_line(&mut pixmap, 0.0, h, w, 0.0, light, color);
            draw_line(&mut pixmap, 0.0, 0.0, w, h, light, color);
        }

        _ => return None,
    }

    Some(pixmap)
}
