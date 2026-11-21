/// Whether shapes other than rectangles are antialiased (feature
/// `programmatic_aa`); rectangles are always pixel-aligned so box lines meet
/// their neighbors exactly.
pub(super) const AA: bool = cfg!(feature = "programmatic_aa");

/// An alpha-only canvas a procedural glyph is drawn into.
pub(crate) struct GlyphCanvas {
    width: u32,
    height: u32,
    data: Vec<u8>,
    /// One shape's coverage before it is merged into `data`.
    tmp: Vec<u8>,
    scratch: zeno::Scratch,
}

impl GlyphCanvas {
    /// A blank canvas, or None for an empty size.
    pub(crate) fn new(width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }
        let len = width as usize * height as usize;
        Some(Self {
            width,
            height,
            data: vec![0; len],
            tmp: vec![0; len],
            scratch: zeno::Scratch::new(),
        })
    }

    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    pub(crate) fn height(&self) -> u32 {
        self.height
    }

    /// Coverage, row by row.
    #[cfg(test)]
    pub(crate) fn data(&self) -> &[u8] {
        &self.data
    }

    /// Index of `(x, y)` in `data`, None outside the canvas.
    fn index(&self, x: u32, y: u32) -> Option<usize> {
        (x < self.width && y < self.height).then(|| y as usize * self.width as usize + x as usize)
    }

    /// Coverage at `(x, y)`, 0 outside the canvas.
    #[cfg(test)]
    pub(crate) fn alpha(&self, x: u32, y: u32) -> u8 {
        self.index(x, y)
            .and_then(|i| self.data.get(i))
            .copied()
            .unwrap_or(0)
    }

    /// Sets the coverage at `(x, y)`; ignored outside the canvas.
    pub(crate) fn set(&mut self, x: u32, y: u32, alpha: u8) {
        if let Some(px) = self.index(x, y).and_then(|i| self.data.get_mut(i)) {
            *px = alpha;
        }
    }

    pub(crate) fn into_coverage(self) -> Vec<u8> {
        self.data
    }

    /// Draws `path` with `style` and merges it in: the larger coverage wins
    /// (thresholded at half coverage without `programmatic_aa`).
    fn shape(&mut self, path: &[zeno::Command], style: zeno::Style, alpha: u8) {
        let finite = crate::utils::outline::all_finite(path);
        if !finite {
            return;
        }
        self.tmp.fill(0);
        zeno::Mask::with_scratch(path, &mut self.scratch)
            .style(style)
            .size(self.width, self.height)
            .render_into(&mut self.tmp, None);
        for (dst, &a) in self.data.iter_mut().zip(self.tmp.iter()) {
            let a = if AA {
                ((u16::from(a) * u16::from(alpha) + 127) / 255) as u8
            } else if a >= 128 {
                alpha
            } else {
                0
            };
            *dst = (*dst).max(a);
        }
    }
}

/// Calculate stroke width based on cell height
pub fn stroke_width(height: u32) -> f32 {
    (height as f32 / 10.0).max(1.0).round()
}

/// Coverage of a fully drawn glyph pixel.
pub fn default_color() -> u8 {
    255
}

/// Fills the pixels whose centers lie in the rectangle, never antialiased.
pub fn draw_rect(canvas: &mut GlyphCanvas, x: f32, y: f32, width: f32, height: f32, alpha: u8) {
    if !(x.is_finite() && y.is_finite() && width.is_finite() && height.is_finite())
        || width <= 0.0
        || height <= 0.0
    {
        return;
    }
    let span = |start: f32, len: f32, limit: u32| {
        let lo = start.round().clamp(0.0, limit as f32) as u32;
        let hi = (start + len).round().clamp(0.0, limit as f32) as u32;
        (lo, hi)
    };
    let (x0, x1) = span(x, width, canvas.width);
    let (y0, y1) = span(y, height, canvas.height);
    let stride = canvas.width as usize;
    for row in y0..y1 {
        let start = row as usize * stride;
        if let Some(px) = canvas
            .data
            .get_mut(start + x0 as usize..start + x1 as usize)
        {
            px.fill(alpha);
        }
    }
}

fn stroke(width: f32) -> zeno::Style<'static> {
    let mut stroke = zeno::Stroke::new(width);
    stroke
        .cap(zeno::Cap::Butt)
        .join(zeno::Join::Miter)
        .miter_limit(4.0);
    stroke.into()
}

/// Draw a stroked line from (x1, y1) to (x2, y2)
pub fn draw_line(
    canvas: &mut GlyphCanvas,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    stroke_width: f32,
    alpha: u8,
) {
    let path = [
        zeno::Command::MoveTo([x1, y1].into()),
        zeno::Command::LineTo([x2, y2].into()),
    ];
    canvas.shape(&path, stroke(stroke_width), alpha);
}

/// Draw a filled triangle
#[allow(clippy::too_many_arguments)]
pub fn draw_triangle(
    canvas: &mut GlyphCanvas,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    x3: f32,
    y3: f32,
    alpha: u8,
) {
    draw_polygon(canvas, &[(x1, y1), (x2, y2), (x3, y3)], alpha);
}

/// Draw a filled circle
pub fn draw_circle(canvas: &mut GlyphCanvas, center_x: f32, center_y: f32, radius: f32, alpha: u8) {
    if radius <= 0.0 {
        return;
    }
    let (cx, cy, r) = (center_x, center_y, radius);
    let k = 0.552_284_8 * r;
    let path = [
        zeno::Command::MoveTo([cx + r, cy].into()),
        zeno::Command::CurveTo(
            [cx + r, cy + k].into(),
            [cx + k, cy + r].into(),
            [cx, cy + r].into(),
        ),
        zeno::Command::CurveTo(
            [cx - k, cy + r].into(),
            [cx - r, cy + k].into(),
            [cx - r, cy].into(),
        ),
        zeno::Command::CurveTo(
            [cx - r, cy - k].into(),
            [cx - k, cy - r].into(),
            [cx, cy - r].into(),
        ),
        zeno::Command::CurveTo(
            [cx + k, cy - r].into(),
            [cx + r, cy - k].into(),
            [cx + r, cy].into(),
        ),
        zeno::Command::Close,
    ];
    canvas.shape(&path, zeno::Fill::NonZero.into(), alpha);
}

/// Draw an arc (for rounded corners)
#[allow(clippy::too_many_arguments)]
pub fn draw_arc(
    canvas: &mut GlyphCanvas,
    center_x: f32,
    center_y: f32,
    radius: f32,
    start_angle: f32,
    end_angle: f32,
    stroke_width: f32,
    alpha: u8,
) {
    let start_rad = start_angle.to_radians();
    let end_rad = end_angle.to_radians();

    let segments = 20;
    let angle_step = (end_rad - start_rad) / segments as f32;

    let mut path = Vec::with_capacity(segments + 1);
    path.push(zeno::Command::MoveTo(
        [
            center_x + radius * start_rad.cos(),
            center_y + radius * start_rad.sin(),
        ]
        .into(),
    ));
    for i in 1..=segments {
        let angle = start_rad + angle_step * i as f32;
        path.push(zeno::Command::LineTo(
            [
                center_x + radius * angle.cos(),
                center_y + radius * angle.sin(),
            ]
            .into(),
        ));
    }
    canvas.shape(&path, stroke(stroke_width), alpha);
}

/// Draw a filled polygon from a list of points
pub fn draw_polygon(canvas: &mut GlyphCanvas, points: &[(f32, f32)], alpha: u8) {
    if points.len() < 3 {
        return;
    }
    let Some((&(x0, y0), rest)) = points.split_first() else {
        return;
    };
    let mut path = Vec::with_capacity(points.len() + 1);
    path.push(zeno::Command::MoveTo([x0, y0].into()));
    for &(x, y) in rest {
        path.push(zeno::Command::LineTo([x, y].into()));
    }
    path.push(zeno::Command::Close);
    canvas.shape(&path, zeno::Fill::NonZero.into(), alpha);
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
        let mut canvas = GlyphCanvas::new(32, 32).unwrap();
        draw_rect(&mut canvas, 0.0, 0.0, 32.0, 32.0, default_color());
        assert!(canvas.data().iter().any(|&a| a > 0));
    }

    #[test]
    fn test_draw_circle() {
        let mut canvas = GlyphCanvas::new(32, 32).unwrap();
        draw_circle(&mut canvas, 16.0, 16.0, 8.0, default_color());
        assert!(canvas.data().iter().any(|&a| a > 0));
    }
}
