use super::box_lines::LineMetrics;
use super::primitives::*;
use tiny_skia::Pixmap;

/// Right-angle triangles U+25E2..=25E5 (filled) and U+25F8..=25FA, U+25FF (outline).
pub fn render(c: char, width: u32, height: u32) -> Option<Pixmap> {
    let (corner_right, corner_bottom, filled) = match c {
        '\u{25E2}' => (true, true, true),
        '\u{25E3}' => (false, true, true),
        '\u{25E4}' => (false, false, true),
        '\u{25E5}' => (true, false, true),
        '\u{25F8}' => (false, false, false),
        '\u{25F9}' => (true, false, false),
        '\u{25FA}' => (false, true, false),
        '\u{25FF}' => (true, true, false),
        _ => return None,
    };
    let mut pixmap = Pixmap::new(width, height)?;
    let (w, h) = (width as f32, height as f32);
    let color = default_color();
    let (cx, cy) = (
        if corner_right { w } else { 0.0 },
        if corner_bottom { h } else { 0.0 },
    );
    if filled {
        draw_triangle(&mut pixmap, cx, cy, w - cx, cy, cx, h - cy, color);
        return Some(pixmap);
    }

    let light = LineMetrics::new(width, height).light as f32;
    let half = light / 2.0;
    let bar_y = if corner_bottom { h - light } else { 0.0 };
    let bar_x = if corner_right { w - light } else { 0.0 };
    draw_rect(&mut pixmap, 0.0, bar_y, w, light, color);
    draw_rect(&mut pixmap, bar_x, 0.0, light, h, color);
    let along_x = if corner_right { half } else { w - half };
    let along_y = if corner_bottom { h - half } else { half };
    let across_x = if corner_right { w - half } else { half };
    let across_y = if corner_bottom { half } else { h - half };
    draw_line(
        &mut pixmap,
        along_x,
        along_y,
        across_x,
        across_y,
        light,
        color,
    );
    Some(pixmap)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn covered(p: &Pixmap, x: u32, y: u32) -> bool {
        p.pixel(x, y).unwrap().alpha() > 0
    }

    #[test]
    fn every_triangle_renders() {
        for c in [
            '\u{25E2}', '\u{25E3}', '\u{25E4}', '\u{25E5}', '\u{25F8}', '\u{25F9}', '\u{25FA}',
            '\u{25FF}',
        ] {
            assert!(render(c, 10, 20).is_some(), "{c:?}");
        }
    }

    #[test]
    fn lower_right_triangle_fills_its_corner_only() {
        let p = render('\u{25E2}', 10, 20).unwrap();
        assert!(covered(&p, 9, 19));
        assert!(!covered(&p, 0, 0));
    }

    #[test]
    fn outlined_triangle_is_hollow() {
        let p = render('\u{25FF}', 10, 20).unwrap();
        assert!(covered(&p, 9, 19));
        assert!(!covered(&p, 2, 4));
        assert!(!covered(&p, 7, 15));
    }
}
