// Primitive drawing functions for programmatic glyph rendering
//
// This module provides low-level drawing functions that are used by the
// specific glyph rendering modules (box_drawing, block_elements, etc.)

use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

/// Calculate stroke width based on cell height
///
/// Follows Rio's convention: height / 10, with a minimum of 1.0 pixel
pub fn stroke_width(height: u32) -> f32 {
    (height as f32 / 10.0).max(1.0).round()
}

/// Get default white color for glyph rendering
pub fn default_color() -> Color {
    Color::from_rgba8(255, 255, 255, 255)
}

/// Draw a filled rectangle
pub fn draw_rect(pixmap: &mut Pixmap, x: f32, y: f32, width: f32, height: f32, color: Color) {
    let mut paint = Paint::default();
    paint.set_color(color);
    // Disable antialiasing to prevent semi-transparent edges that create gaps
    paint.anti_alias = false;

    if let Some(rect) = tiny_skia::Rect::from_xywh(x, y, width, height) {
        let path = PathBuilder::from_rect(rect);
        pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

/// Draw a horizontal line across the full width of the pixmap
/// Note: Goes exactly from edge to edge (0 to width) for pixel-perfect alignment
pub fn draw_horizontal_line(pixmap: &mut Pixmap, y: f32, stroke: f32, color: Color) {
    draw_rect(
        pixmap,
        0.0,
        y - stroke / 2.0,
        pixmap.width() as f32,
        stroke,
        color,
    );
}

/// Draw a vertical line across the full height of the pixmap
/// Note: Goes exactly from edge to edge (0 to height) for pixel-perfect alignment
pub fn draw_vertical_line(pixmap: &mut Pixmap, x: f32, stroke: f32, color: Color) {
    draw_rect(
        pixmap,
        x - stroke / 2.0,
        0.0,
        stroke,
        pixmap.height() as f32,
        color,
    );
}

/// Draw a stroked line from (x1, y1) to (x2, y2)
pub fn draw_line(
    pixmap: &mut Pixmap,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    stroke_width: f32,
    color: Color,
) {
    let mut paint = Paint::default();
    paint.set_color(color);
    // Disable antialiasing for pixel-perfect edges
    paint.anti_alias = false;

    let mut pb = PathBuilder::new();
    pb.move_to(x1, y1);
    pb.line_to(x2, y2);

    if let Some(path) = pb.finish() {
        let stroke = Stroke {
            width: stroke_width,
            ..Default::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

/// Draw a filled triangle
#[allow(clippy::too_many_arguments)]
pub fn draw_triangle(
    pixmap: &mut Pixmap,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    x3: f32,
    y3: f32,
    color: Color,
) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = false;

    let mut pb = PathBuilder::new();
    pb.move_to(x1, y1);
    pb.line_to(x2, y2);
    pb.line_to(x3, y3);
    pb.close();

    if let Some(path) = pb.finish() {
        pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

/// Draw a filled circle
pub fn draw_circle(pixmap: &mut Pixmap, center_x: f32, center_y: f32, radius: f32, color: Color) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = false;

    let mut pb = PathBuilder::new();
    pb.push_circle(center_x, center_y, radius);

    if let Some(path) = pb.finish() {
        pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

/// Draw an arc (for rounded corners)
///
/// Angles are in degrees (0° = right, 90° = down, 180° = left, 270° = up)
#[allow(clippy::too_many_arguments)]
pub fn draw_arc(
    pixmap: &mut Pixmap,
    center_x: f32,
    center_y: f32,
    radius: f32,
    start_angle: f32,
    end_angle: f32,
    stroke_width: f32,
    color: Color,
) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = false;

    let mut pb = PathBuilder::new();
    // Convert angles to radians
    let start_rad = start_angle.to_radians();
    let end_rad = end_angle.to_radians();

    // Draw arc using line segments (tiny-skia doesn't have native arc)
    let segments = 20;
    let angle_step = (end_rad - start_rad) / segments as f32;

    pb.move_to(
        center_x + radius * start_rad.cos(),
        center_y + radius * start_rad.sin(),
    );

    for i in 1..=segments {
        let angle = start_rad + angle_step * i as f32;
        pb.line_to(
            center_x + radius * angle.cos(),
            center_y + radius * angle.sin(),
        );
    }

    if let Some(path) = pb.finish() {
        let stroke = Stroke {
            width: stroke_width,
            ..Default::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

/// Draw a filled polygon from a list of points
pub fn draw_polygon(pixmap: &mut Pixmap, points: &[(f32, f32)], color: Color) {
    if points.len() < 3 {
        return; // Need at least 3 points for a polygon
    }

    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = false;

    let mut pb = PathBuilder::new();
    pb.move_to(points[0].0, points[0].1);

    for &(x, y) in &points[1..] {
        pb.line_to(x, y);
    }
    pb.close();

    if let Some(path) = pb.finish() {
        pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stroke_width() {
        assert_eq!(stroke_width(10), 1.0);
        assert_eq!(stroke_width(20), 2.0);
        assert_eq!(stroke_width(100), 10.0);
    }

    #[test]
    fn test_draw_rect() {
        let mut pixmap = Pixmap::new(32, 32).unwrap();
        draw_rect(&mut pixmap, 0.0, 0.0, 32.0, 32.0, default_color());
        // Should have drawn pixels
        assert!(pixmap.pixels().iter().any(|p| p.alpha() > 0));
    }

    #[test]
    fn test_draw_circle() {
        let mut pixmap = Pixmap::new(32, 32).unwrap();
        draw_circle(&mut pixmap, 16.0, 16.0, 8.0, default_color());
        // Should have drawn pixels
        assert!(pixmap.pixels().iter().any(|p| p.alpha() > 0));
    }
}
