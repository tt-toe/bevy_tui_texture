//! Glyph outlines collected as zeno path commands.

use rustybuzz::ttf_parser::OutlineBuilder;

/// Appends an outline, in font units, to a zeno command list.
pub(crate) struct OutlineSink<'a>(pub(crate) &'a mut Vec<zeno::Command>);

impl OutlineBuilder for OutlineSink<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push(zeno::Command::MoveTo([x, y].into()));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push(zeno::Command::LineTo([x, y].into()));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0
            .push(zeno::Command::QuadTo([x1, y1].into(), [x, y].into()));
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.push(zeno::Command::CurveTo(
            [x1, y1].into(),
            [x2, y2].into(),
            [x, y].into(),
        ));
    }

    fn close(&mut self) {
        self.0.push(zeno::Command::Close);
    }
}

/// Whether every point of `path` is finite (zeno must never see one that
/// is not).
pub(crate) fn all_finite(path: &[zeno::Command]) -> bool {
    let ok = |p: &zeno::Vector| p.x.is_finite() && p.y.is_finite();
    path.iter().all(|command| match command {
        zeno::Command::MoveTo(p) | zeno::Command::LineTo(p) => ok(p),
        zeno::Command::QuadTo(c, p) => ok(c) && ok(p),
        zeno::Command::CurveTo(c1, c2, p) => ok(c1) && ok(c2) && ok(p),
        zeno::Command::Close => true,
    })
}

/// Passes every point of `path` through `map`; false if any result is not
/// finite.
pub(crate) fn map_points(
    path: &mut [zeno::Command],
    mut map: impl FnMut(zeno::Vector) -> zeno::Vector,
) -> bool {
    for command in path.iter_mut() {
        match command {
            zeno::Command::MoveTo(p) | zeno::Command::LineTo(p) => *p = map(*p),
            zeno::Command::QuadTo(c, p) => {
                *c = map(*c);
                *p = map(*p);
            }
            zeno::Command::CurveTo(c1, c2, p) => {
                *c1 = map(*c1);
                *c2 = map(*c2);
                *p = map(*p);
            }
            zeno::Command::Close => {}
        }
    }
    all_finite(path)
}
