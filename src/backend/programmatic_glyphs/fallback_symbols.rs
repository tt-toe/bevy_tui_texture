use super::box_lines::LineMetrics;
use super::primitives::*;
use tiny_skia::Pixmap;

/// Symbols ratatui widgets emit that are drawn procedurally when no font has them.
pub(crate) fn is_fallback_symbol(c: char) -> bool {
    matches!(
        c,
        '←' | '↑' | '→' | '↓' | '▲' | '►' | '▼' | '◄' | '▰' | '▱' | '•' | '×'
    )
}

/// Whether the glyph a font shaped `ch` to is its `.notdef` and `ch` has a procedural form.
pub(crate) fn use_fallback(glyph_id: u32, ch: char) -> bool {
    glyph_id == 0 && is_fallback_symbol(ch)
}

pub(crate) fn render(c: char, width: u32, height: u32) -> Option<Pixmap> {
    if !is_fallback_symbol(c) {
        return None;
    }
    let mut pixmap = Pixmap::new(width, height)?;
    let (w, h) = (width as f32, height as f32);
    let light = LineMetrics::new(width, height).light as f32;
    let color = default_color();
    let (mx, my) = (w * 0.1, h * 0.1);
    let (cx, cy) = (w / 2.0, h / 2.0);
    match c {
        '→' | '←' => {
            let dir = if c == '→' { 1.0 } else { -1.0 };
            let (tail, tip) = (cx - dir * (cx - mx), cx + dir * (cx - mx));
            let base = tip - dir * w * 0.4;
            let half = (h * 0.2).max(light);
            draw_triangle(
                &mut pixmap,
                tip,
                cy,
                base,
                cy - half,
                base,
                cy + half,
                color,
            );
            let (x0, x1) = (tail.min(base), tail.max(base));
            draw_rect(&mut pixmap, x0, cy - light / 2.0, x1 - x0, light, color);
        }
        '↑' | '↓' => {
            let dir = if c == '↓' { 1.0 } else { -1.0 };
            let (tail, tip) = (cy - dir * (cy - my), cy + dir * (cy - my));
            let base = tip - dir * h * 0.25;
            let half = (w * 0.3).max(light);
            draw_triangle(
                &mut pixmap,
                cx,
                tip,
                cx - half,
                base,
                cx + half,
                base,
                color,
            );
            let (y0, y1) = (tail.min(base), tail.max(base));
            draw_rect(&mut pixmap, cx - light / 2.0, y0, light, y1 - y0, color);
        }
        '▲' | '▼' => {
            let (half, len) = (w * 0.4, (w * 0.6).min(h * 0.8));
            let (near, far) = (cy - len / 2.0, cy + len / 2.0);
            if c == '▲' {
                draw_triangle(&mut pixmap, cx, near, cx - half, far, cx + half, far, color);
            } else {
                draw_triangle(
                    &mut pixmap,
                    cx,
                    far,
                    cx - half,
                    near,
                    cx + half,
                    near,
                    color,
                );
            }
        }
        '►' | '◄' => {
            let (half, len) = ((w * 0.4).min(h * 0.4), w * 0.6);
            let (near, far) = (cx - len / 2.0, cx + len / 2.0);
            if c == '►' {
                draw_triangle(
                    &mut pixmap,
                    far,
                    cy,
                    near,
                    cy - half,
                    near,
                    cy + half,
                    color,
                );
            } else {
                draw_triangle(&mut pixmap, near, cy, far, cy - half, far, cy + half, color);
            }
        }
        '▰' => draw_rect(&mut pixmap, mx, h * 0.3, w * 0.8, h * 0.4, color),
        '▱' => {
            let (x, y, bw, bh) = (mx, h * 0.3, w * 0.8, h * 0.4);
            let t = light.min(bh / 2.0).min(bw / 2.0);
            draw_rect(&mut pixmap, x, y, bw, t, color);
            draw_rect(&mut pixmap, x, y + bh - t, bw, t, color);
            draw_rect(&mut pixmap, x, y, t, bh, color);
            draw_rect(&mut pixmap, x + bw - t, y, t, bh, color);
        }
        '•' => draw_circle(&mut pixmap, cx, cy, w.min(h) * 0.2, color),
        _ => {
            let half = w * 0.3;
            draw_line(
                &mut pixmap,
                cx - half,
                cy - half,
                cx + half,
                cy + half,
                light,
                color,
            );
            draw_line(
                &mut pixmap,
                cx - half,
                cy + half,
                cx + half,
                cy - half,
                light,
                color,
            );
        }
    }
    Some(pixmap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_symbols_all_render() {
        for c in "←↑→↓▲►▼◄▰▱•×".chars() {
            for (w, h) in [(7, 14), (10, 20), (11, 23)] {
                let p = render(c, w, h).unwrap();
                assert!(p.pixels().iter().any(|px| px.alpha() > 0), "{c:?} {w}x{h}");
            }
        }
        assert!(render('a', 10, 20).is_none());
    }

    #[test]
    fn fallback_only_for_notdef() {
        assert!(use_fallback(0, '▲'));
        assert!(!use_fallback(5, '▲'));
        assert!(!use_fallback(0, 'a'));
    }
}
