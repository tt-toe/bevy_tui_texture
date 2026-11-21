//! COLR (v0 and v1) glyph painting on zeno coverage masks.

use rustybuzz::Face;
use rustybuzz::ttf_parser::colr::{self, CompositeMode, GradientExtend};
use rustybuzz::ttf_parser::{GlyphId, RgbaColor};

use crate::utils::outline::{OutlineSink, map_points};

/// 2D affine map: `x' = a*x + c*y + e`, `y' = b*x + d*y + f`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Affine {
    pub(crate) a: f32,
    pub(crate) b: f32,
    pub(crate) c: f32,
    pub(crate) d: f32,
    pub(crate) e: f32,
    pub(crate) f: f32,
}

impl Affine {
    pub(crate) const IDENTITY: Self = Self::new(1.0, 0.0, 0.0, 1.0, 0.0, 0.0);

    pub(crate) const fn new(a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) -> Self {
        Self { a, b, c, d, e, f }
    }

    pub(crate) const fn scale(x: f32, y: f32) -> Self {
        Self::new(x, 0.0, 0.0, y, 0.0, 0.0)
    }

    /// `self` first, then `next`.
    pub(crate) fn then(&self, next: &Self) -> Self {
        Self {
            a: self.a * next.a + self.b * next.c,
            b: self.a * next.b + self.b * next.d,
            c: self.c * next.a + self.d * next.c,
            d: self.c * next.b + self.d * next.d,
            e: self.e * next.a + self.f * next.c + next.e,
            f: self.e * next.b + self.f * next.d + next.f,
        }
    }

    pub(crate) fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    fn inverse(&self) -> Option<Self> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < f32::EPSILON * 1e-3 || !det.is_finite() {
            return None;
        }
        let (a, b, c, d) = (self.d / det, -self.b / det, -self.c / det, self.a / det);
        Some(Self {
            a,
            b,
            c,
            d,
            e: -(self.e * a + self.f * c),
            f: -(self.e * b + self.f * d),
        })
    }
}

impl From<rustybuzz::ttf_parser::Transform> for Affine {
    fn from(t: rustybuzz::ttf_parser::Transform) -> Self {
        Self::new(t.a, t.b, t.c, t.d, t.e, t.f)
    }
}

/// Premultiplied RGBA, each channel in `0..=1`.
type Rgba = [f32; 4];

/// A COLR paint resolved to the canvas: a color for each pixel center.
enum Source {
    Solid(Rgba),
    Linear {
        stops: Vec<(f32, [f32; 4])>,
        extend: GradientExtend,
        p0: (f32, f32),
        d: (f32, f32),
    },
    Radial {
        stops: Vec<(f32, [f32; 4])>,
        extend: GradientExtend,
        to_glyph: Affine,
        c0: (f32, f32),
        r0: f32,
        c1: (f32, f32),
        r1: f32,
    },
    Sweep {
        stops: Vec<(f32, [f32; 4])>,
        extend: GradientExtend,
        to_glyph: Affine,
        center: (f32, f32),
        start: f32,
        end: f32,
    },
}

fn unpremultiplied(color: RgbaColor) -> [f32; 4] {
    [
        f32::from(color.red) / 255.0,
        f32::from(color.green) / 255.0,
        f32::from(color.blue) / 255.0,
        f32::from(color.alpha) / 255.0,
    ]
}

fn premultiply([r, g, b, a]: [f32; 4]) -> Rgba {
    [r * a, g * a, b * a, a]
}

fn extend(t: f32, mode: GradientExtend) -> f32 {
    match mode {
        GradientExtend::Pad => t.clamp(0.0, 1.0),
        GradientExtend::Repeat => t - t.floor(),
        GradientExtend::Reflect => {
            let t = (t / 2.0 - (t / 2.0).floor()) * 2.0;
            if t > 1.0 { 2.0 - t } else { t }
        }
    }
}

/// Color at `t` of sorted stops, interpolated unpremultiplied and
/// premultiplied afterwards.
fn stop_color(stops: &[(f32, [f32; 4])], t: f32) -> Rgba {
    let (Some(first), Some(last)) = (stops.first(), stops.last()) else {
        return [0.0; 4];
    };
    if t <= first.0 {
        return premultiply(first.1);
    }
    if t >= last.0 {
        return premultiply(last.1);
    }
    for pair in stops.windows(2) {
        if let [(t0, c0), (t1, c1)] = pair
            && t >= *t0
            && t <= *t1
        {
            let span = t1 - t0;
            let k = if span > 0.0 { (t - t0) / span } else { 1.0 };
            let mut c = [0.0; 4];
            for ((c, a), b) in c.iter_mut().zip(c0).zip(c1) {
                *c = a + (b - a) * k;
            }
            return premultiply(c);
        }
    }
    premultiply(last.1)
}

impl Source {
    fn color_at(&self, x: f32, y: f32) -> Rgba {
        match self {
            Source::Solid(color) => *color,
            Source::Linear {
                stops,
                extend: mode,
                p0,
                d,
            } => {
                let len = d.0 * d.0 + d.1 * d.1;
                let t = ((x - p0.0) * d.0 + (y - p0.1) * d.1) / len;
                stop_color(stops, extend(t, *mode))
            }
            Source::Radial {
                stops,
                extend: mode,
                to_glyph,
                c0,
                r0,
                c1,
                r1,
            } => {
                let (px, py) = to_glyph.apply(x, y);
                match conical_t((px, py), *c0, *r0, *c1, *r1) {
                    Some(t) => stop_color(stops, extend(t, *mode)),
                    None => [0.0; 4],
                }
            }
            Source::Sweep {
                stops,
                extend: mode,
                to_glyph,
                center,
                start,
                end,
            } => {
                let (px, py) = to_glyph.apply(x, y);
                let mut angle = (py - center.1).atan2(px - center.0).to_degrees();
                if angle < 0.0 {
                    angle += 360.0;
                }
                let span = end - start;
                if span.abs() < f32::EPSILON {
                    return [0.0; 4];
                }
                stop_color(stops, extend((angle - start) / span, *mode))
            }
        }
    }
}

/// Largest `t` with `|p - c(t)| = r(t)` and `r(t) >= 0` for the two-point
/// conical gradient from circle `(c0, r0)` to `(c1, r1)`.
fn conical_t(p: (f32, f32), c0: (f32, f32), r0: f32, c1: (f32, f32), r1: f32) -> Option<f32> {
    let cd = (c1.0 - c0.0, c1.1 - c0.1);
    let pd = (p.0 - c0.0, p.1 - c0.1);
    let dr = r1 - r0;
    let a = cd.0 * cd.0 + cd.1 * cd.1 - dr * dr;
    let b = pd.0 * cd.0 + pd.1 * cd.1 + r0 * dr;
    let c = pd.0 * pd.0 + pd.1 * pd.1 - r0 * r0;
    let radius_ok = |t: f32| r0 + t * dr >= 0.0;
    if a.abs() < 1e-6 {
        if b.abs() < 1e-6 {
            return None;
        }
        let t = c / (2.0 * b);
        return radius_ok(t).then_some(t);
    }
    let disc = b * b - a * c;
    if disc < 0.0 {
        return None;
    }
    let root = disc.sqrt();
    let (t1, t2) = ((b + root) / a, (b - root) / a);
    let (hi, lo) = if t1 > t2 { (t1, t2) } else { (t2, t1) };
    if radius_ok(hi) {
        Some(hi)
    } else if radius_ok(lo) {
        Some(lo)
    } else {
        None
    }
}

fn sorted_stops(stops: impl Iterator<Item = colr::ColorStop>) -> Vec<(f32, [f32; 4])> {
    let mut stops: Vec<_> = stops
        .map(|stop| (stop.stop_offset, unpremultiplied(stop.color)))
        .collect();
    stops.sort_by(|l, r| l.0.total_cmp(&r.0));
    stops
}

fn lum([r, g, b]: [f32; 3]) -> f32 {
    0.3 * r + 0.59 * g + 0.11 * b
}

fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut out = c;
    if n < 0.0 {
        let k = l - n;
        for v in &mut out {
            *v = if k > 0.0 { l + (*v - l) * l / k } else { l };
        }
    }
    if x > 1.0 {
        let k = x - l;
        for v in &mut out {
            *v = if k > 0.0 {
                l + (*v - l) * (1.0 - l) / k
            } else {
                l
            };
        }
    }
    out
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    let range = max - min;
    let mut out = [0.0; 3];
    if range > 0.0 {
        for (o, v) in out.iter_mut().zip(c) {
            *o = (v - min) * s / range;
        }
    }
    out
}

/// `B(Cb, Cs)` of a separable blend mode, on unpremultiplied channels.
fn separable(mode: CompositeMode, cb: f32, cs: f32) -> f32 {
    let hard_light = |cb: f32, cs: f32| {
        if cs <= 0.5 {
            cb * 2.0 * cs
        } else {
            let s = 2.0 * cs - 1.0;
            cb + s - cb * s
        }
    };
    match mode {
        CompositeMode::Screen => cb + cs - cb * cs,
        CompositeMode::Overlay => hard_light(cs, cb),
        CompositeMode::Darken => cb.min(cs),
        CompositeMode::Lighten => cb.max(cs),
        CompositeMode::ColorDodge => {
            if cb <= 0.0 {
                0.0
            } else if cs >= 1.0 {
                1.0
            } else {
                (cb / (1.0 - cs)).min(1.0)
            }
        }
        CompositeMode::ColorBurn => {
            if cb >= 1.0 {
                1.0
            } else if cs <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - cb) / cs).min(1.0)
            }
        }
        CompositeMode::HardLight => hard_light(cb, cs),
        CompositeMode::SoftLight => {
            if cs <= 0.5 {
                cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
            } else {
                let d = if cb <= 0.25 {
                    ((16.0 * cb - 12.0) * cb + 4.0) * cb
                } else {
                    cb.sqrt()
                };
                cb + (2.0 * cs - 1.0) * (d - cb)
            }
        }
        CompositeMode::Difference => (cb - cs).abs(),
        CompositeMode::Exclusion => cb + cs - 2.0 * cb * cs,
        CompositeMode::Multiply => cb * cs,
        _ => cs,
    }
}

/// `s` composited onto `d` (both premultiplied) with `mode`.
fn composite(mode: CompositeMode, s: Rgba, d: Rgba) -> Rgba {
    let (sa, da) = (s[3], d[3]);
    let pd = |fs: f32, fd: f32| {
        let mut out = [0.0; 4];
        for ((o, s), d) in out.iter_mut().zip(s).zip(d) {
            *o = s * fs + d * fd;
        }
        out
    };
    match mode {
        CompositeMode::Clear => [0.0; 4],
        CompositeMode::Source => s,
        CompositeMode::Destination => d,
        CompositeMode::SourceOver => pd(1.0, 1.0 - sa),
        CompositeMode::DestinationOver => pd(1.0 - da, 1.0),
        CompositeMode::SourceIn => pd(da, 0.0),
        CompositeMode::DestinationIn => pd(0.0, sa),
        CompositeMode::SourceOut => pd(1.0 - da, 0.0),
        CompositeMode::DestinationOut => pd(0.0, 1.0 - sa),
        CompositeMode::SourceAtop => pd(da, 1.0 - sa),
        CompositeMode::DestinationAtop => pd(1.0 - da, sa),
        CompositeMode::Xor => pd(1.0 - da, 1.0 - sa),
        CompositeMode::Plus => {
            let mut out = [0.0; 4];
            for ((o, s), d) in out.iter_mut().zip(s).zip(d) {
                *o = (s + d).min(1.0);
            }
            out
        }
        _ => {
            let un = |c: Rgba| -> [f32; 3] {
                if c[3] > 0.0 {
                    [c[0] / c[3], c[1] / c[3], c[2] / c[3]]
                } else {
                    [0.0; 3]
                }
            };
            let (cs, cb) = (un(s), un(d));
            let blended = match mode {
                CompositeMode::Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
                CompositeMode::Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
                CompositeMode::Color => set_lum(cs, lum(cb)),
                CompositeMode::Luminosity => set_lum(cb, lum(cs)),
                _ => [
                    separable(mode, cb[0], cs[0]),
                    separable(mode, cb[1], cs[1]),
                    separable(mode, cb[2], cs[2]),
                ],
            };
            let mut out = [0.0, 0.0, 0.0, sa + da - sa * da];
            for (((o, s), d), bl) in out.iter_mut().zip(s).zip(d).zip(blended) {
                *o = s * (1.0 - da) + d * (1.0 - sa) + sa * da * bl;
            }
            out
        }
    }
}

/// A `ttf_parser` COLR painter drawing into a `width` x `height` canvas.
pub(crate) struct ColorPainter<'f, 'd> {
    face: &'f Face<'d>,
    width: usize,
    height: usize,
    /// Font units -> canvas pixels, applied after the glyph's own transforms.
    base: Affine,
    transforms: Vec<Affine>,
    outline: Option<Vec<zeno::Command>>,
    layers: Vec<Vec<Rgba>>,
    modes: Vec<CompositeMode>,
    clips: Vec<Vec<u8>>,
    scratch: zeno::Scratch,
    mask: Vec<u8>,
}

impl<'f, 'd> ColorPainter<'f, 'd> {
    pub(crate) fn new(face: &'f Face<'d>, width: usize, height: usize, base: Affine) -> Self {
        Self {
            face,
            width,
            height,
            base,
            transforms: Vec::new(),
            outline: None,
            layers: vec![vec![[0.0; 4]; width * height]],
            modes: Vec::new(),
            clips: Vec::new(),
            scratch: zeno::Scratch::new(),
            mask: Vec::new(),
        }
    }

    /// Paints `glyph`, or None if the font has no color data for it.
    pub(crate) fn paint_glyph(mut self, glyph: GlyphId) -> Option<Vec<Rgba>> {
        let face = self.face;
        face.paint_color_glyph(glyph, 0, RgbaColor::new(255, 255, 255, 255), &mut self)?;
        self.layers.into_iter().next()
    }

    fn transform(&self) -> Affine {
        self.transforms
            .iter()
            .rev()
            .fold(Affine::IDENTITY, |acc, t| acc.then(t))
            .then(&self.base)
    }

    /// Coverage of `path` (font units) under the current transform, into
    /// `self.mask`.
    fn rasterize(&mut self, path: &[zeno::Command]) {
        let map = self.transform();
        let mut mapped = path.to_vec();
        let finite = map_points(&mut mapped, |p| {
            let (x, y) = map.apply(p.x, p.y);
            zeno::Vector::new(x, y)
        });
        self.mask.clear();
        self.mask.resize(self.width * self.height, 0);
        if !finite || self.width == 0 || self.height == 0 {
            return;
        }
        zeno::Mask::with_scratch(&mapped[..], &mut self.scratch)
            .style(zeno::Fill::NonZero)
            .size(self.width as u32, self.height as u32)
            .render_into(&mut self.mask, None);
    }

    fn top_clip(&self) -> Option<&Vec<u8>> {
        self.clips.last()
    }

    fn push_mask(&mut self, mut mask: Vec<u8>) {
        if let Some(clip) = self.top_clip() {
            for (m, &c) in mask.iter_mut().zip(clip.iter()) {
                *m = ((u16::from(*m) * u16::from(c) + 127) / 255) as u8;
            }
        }
        self.clips.push(mask);
    }

    fn source(&self, paint: colr::Paint<'_>) -> Option<Source> {
        let transform = self.transform();
        Some(match paint {
            colr::Paint::Solid(color) => Source::Solid(premultiply(unpremultiplied(color))),
            colr::Paint::LinearGradient(grad) => {
                let stops = sorted_stops(grad.stops(0, &[]));
                let p0 = transform.apply(grad.x0, grad.y0);
                let p1 = transform.apply(grad.x1, grad.y1);
                let p2 = transform.apply(grad.x2, grad.y2);
                if p0 == p1 || p0 == p2 {
                    return None;
                }
                let perp = (p2.1 - p0.1, -(p2.0 - p0.0));
                let dist = (p1.0 - p0.0, p1.1 - p0.1);
                let perp_len = perp.0 * perp.0 + perp.1 * perp.1;
                let k = (dist.0 * perp.0 + dist.1 * perp.1) / perp_len.max(f32::MIN_POSITIVE);
                Source::Linear {
                    stops,
                    extend: grad.extend,
                    p0,
                    d: (perp.0 * k, perp.1 * k),
                }
            }
            colr::Paint::RadialGradient(grad) => {
                if grad.x0 == grad.x1 && grad.y0 == grad.y1 && grad.r0 == grad.r1 {
                    return None;
                }
                Source::Radial {
                    stops: sorted_stops(grad.stops(0, &[])),
                    extend: grad.extend,
                    to_glyph: transform.inverse()?,
                    c0: (grad.x0, grad.y0),
                    r0: grad.r0,
                    c1: (grad.x1, grad.y1),
                    r1: grad.r1,
                }
            }
            colr::Paint::SweepGradient(grad) => Source::Sweep {
                stops: sorted_stops(grad.stops(0, &[])),
                extend: grad.extend,
                to_glyph: transform.inverse()?,
                center: (grad.center_x, grad.center_y),
                start: grad.start_angle * 180.0,
                end: grad.end_angle * 180.0,
            },
        })
    }
}

impl<'a> colr::Painter<'a> for ColorPainter<'_, '_> {
    fn outline_glyph(&mut self, glyph_id: GlyphId) {
        let mut path = Vec::new();
        self.outline = self
            .face
            .outline_glyph(glyph_id, &mut OutlineSink(&mut path))
            .map(|_| path);
    }

    fn paint(&mut self, paint: colr::Paint<'a>) {
        let outline = self.outline.take();
        let Some(source) = self.source(paint) else {
            return;
        };
        match outline {
            Some(path) => self.rasterize(&path),
            None => {
                self.mask.clear();
                self.mask.resize(self.width * self.height, 255);
            }
        }
        let width = self.width.max(1);
        let mask = std::mem::take(&mut self.mask);
        let clip = self.clips.last();
        if let Some(layer) = self.layers.last_mut() {
            for (i, (dst, &m)) in layer.iter_mut().zip(mask.iter()).enumerate() {
                let c = clip.and_then(|clip| clip.get(i)).copied().unwrap_or(255);
                let coverage = f32::from(m) * f32::from(c) / (255.0 * 255.0);
                if coverage <= 0.0 {
                    continue;
                }
                let (x, y) = ((i % width) as f32 + 0.5, (i / width) as f32 + 0.5);
                let mut s = source.color_at(x, y);
                for v in &mut s {
                    *v *= coverage;
                }
                *dst = composite(CompositeMode::SourceOver, s, *dst);
            }
        }
        self.mask = mask;
    }

    fn push_clip(&mut self) {
        let path = self.outline.take().unwrap_or_default();
        self.rasterize(&path);
        let mask = std::mem::take(&mut self.mask);
        self.push_mask(mask);
    }

    fn push_clip_box(&mut self, clipbox: colr::ClipBox) {
        let t = self.transform();
        let corners = [
            t.apply(clipbox.x_min, clipbox.y_min),
            t.apply(clipbox.x_max, clipbox.y_max),
            t.apply(clipbox.x_min, clipbox.y_max),
            t.apply(clipbox.x_max, clipbox.y_min),
        ];
        let fold = |f: fn(f32, f32) -> f32, axis: fn(&(f32, f32)) -> f32, init: f32| {
            corners.iter().map(axis).fold(init, f)
        };
        let to_px = |v: f32, limit: usize| (v as i64).clamp(0, limit as i64) as usize;
        let x0 = to_px(fold(f32::min, |p| p.0, f32::INFINITY), self.width);
        let x1 = to_px(fold(f32::max, |p| p.0, f32::NEG_INFINITY), self.width);
        let y0 = to_px(fold(f32::min, |p| p.1, f32::INFINITY), self.height);
        let y1 = to_px(fold(f32::max, |p| p.1, f32::NEG_INFINITY), self.height);
        let mut mask = vec![0u8; self.width * self.height];
        for y in y0..y1 {
            if let Some(row) = mask.get_mut(y * self.width + x0..y * self.width + x1) {
                row.fill(255);
            }
        }
        self.push_mask(mask);
    }

    fn pop_clip(&mut self) {
        self.clips.pop();
    }

    fn push_layer(&mut self, mode: CompositeMode) {
        self.layers.push(vec![[0.0; 4]; self.width * self.height]);
        self.modes.push(mode);
    }

    fn pop_layer(&mut self) {
        let mode = self.modes.pop().unwrap_or(CompositeMode::SourceOver);
        if self.layers.len() < 2 {
            return;
        }
        let Some(layer) = self.layers.pop() else {
            return;
        };
        let clip = self.clips.last();
        if let Some(dst) = self.layers.last_mut() {
            for (i, (d, s)) in dst.iter_mut().zip(layer).enumerate() {
                let c = clip.and_then(|clip| clip.get(i)).copied().unwrap_or(255);
                if c == 0 {
                    continue;
                }
                let blended = composite(mode, s, *d);
                let k = f32::from(c) / 255.0;
                for (out, b) in d.iter_mut().zip(blended) {
                    *out += (b - *out) * k;
                }
            }
        }
    }

    fn push_transform(&mut self, transform: rustybuzz::ttf_parser::Transform) {
        self.transforms.push(transform.into());
    }

    fn pop_transform(&mut self) {
        self.transforms.pop();
    }
}

/// A `0..=1` channel as a byte.
fn unit_byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Premultiplied RGBA8 bytes of a painted canvas.
#[cfg(feature = "emoji")]
pub(crate) fn to_rgba8(pixels: &[Rgba]) -> Vec<u8> {
    pixels.iter().flat_map(|p| p.map(unit_byte)).collect()
}

/// Alpha bytes of a painted canvas.
pub(crate) fn to_alpha(pixels: &[Rgba]) -> Vec<u8> {
    pixels.iter().map(|&[.., a]| unit_byte(a)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: Rgba = [0.2, 0.1, 0.0, 0.5];
    const D: Rgba = [0.0, 0.3, 0.6, 0.75];

    fn close(a: Rgba, b: Rgba) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5)
    }

    #[test]
    fn porter_duff_modes_follow_their_formulas() {
        let cases = [
            (CompositeMode::Clear, [0.0; 4]),
            (CompositeMode::Source, S),
            (CompositeMode::Destination, D),
            (CompositeMode::SourceOver, [0.2, 0.25, 0.3, 0.875]),
            (CompositeMode::DestinationOver, [0.05, 0.325, 0.6, 0.875]),
            (CompositeMode::SourceIn, [0.15, 0.075, 0.0, 0.375]),
            (CompositeMode::DestinationIn, [0.0, 0.15, 0.3, 0.375]),
            (CompositeMode::SourceOut, [0.05, 0.025, 0.0, 0.125]),
            (CompositeMode::DestinationOut, [0.0, 0.15, 0.3, 0.375]),
            (CompositeMode::SourceAtop, [0.15, 0.225, 0.3, 0.75]),
            (CompositeMode::DestinationAtop, [0.05, 0.175, 0.3, 0.5]),
            (CompositeMode::Xor, [0.05, 0.175, 0.3, 0.5]),
            (CompositeMode::Plus, [0.2, 0.4, 0.6, 1.0]),
        ];
        for (mode, want) in cases {
            let got = composite(mode, S, D);
            assert!(close(got, want), "{mode:?}: {got:?} != {want:?}");
        }
    }

    #[test]
    fn blend_modes_over_opaque_colors_reduce_to_their_blend_function() {
        let (s, d) = ([0.25, 0.5, 0.75, 1.0], [0.5, 0.5, 0.5, 1.0]);
        let cases = [
            (CompositeMode::Multiply, [0.125, 0.25, 0.375]),
            (CompositeMode::Screen, [0.625, 0.75, 0.875]),
            (CompositeMode::Darken, [0.25, 0.5, 0.5]),
            (CompositeMode::Lighten, [0.5, 0.5, 0.75]),
            (CompositeMode::Difference, [0.25, 0.0, 0.25]),
            (CompositeMode::Exclusion, [0.5, 0.5, 0.5]),
            (CompositeMode::Overlay, [0.25, 0.5, 0.75]),
            (CompositeMode::HardLight, [0.25, 0.5, 0.75]),
            (CompositeMode::Luminosity, [0.4525, 0.4525, 0.4525]),
        ];
        for (mode, want) in cases {
            let got = composite(mode, s, d);
            assert!((got[3] - 1.0).abs() < 1e-5, "{mode:?} alpha");
            for i in 0..3 {
                assert!(
                    (got[i] - want[i]).abs() < 1e-3,
                    "{mode:?}[{i}]: {got:?} != {want:?}"
                );
            }
        }
        for mode in [
            CompositeMode::ColorDodge,
            CompositeMode::ColorBurn,
            CompositeMode::SoftLight,
            CompositeMode::Hue,
            CompositeMode::Saturation,
            CompositeMode::Color,
        ] {
            let got = composite(mode, s, d);
            assert!(
                got.iter().all(|v| (0.0..=1.0).contains(v)),
                "{mode:?}: {got:?}"
            );
        }
    }

    #[test]
    fn a_transform_and_its_inverse_cancel() {
        let t = Affine::new(2.0, 0.5, -0.25, 3.0, 4.0, -7.0);
        let back = t.then(&t.inverse().expect("invertible"));
        let (x, y) = back.apply(3.0, 5.0);
        assert!((x - 3.0).abs() < 1e-4 && (y - 5.0).abs() < 1e-4);
    }

    #[test]
    fn a_conical_gradient_between_concentric_circles_is_radial() {
        let t = conical_t((5.0, 0.0), (0.0, 0.0), 0.0, (0.0, 0.0), 10.0);
        assert!(t.is_some_and(|t| (t - 0.5).abs() < 1e-5));
    }

    #[test]
    fn gradient_extend_modes_wrap_t() {
        assert_eq!(extend(1.5, GradientExtend::Pad), 1.0);
        assert!((extend(1.25, GradientExtend::Repeat) - 0.25).abs() < 1e-6);
        assert!((extend(1.25, GradientExtend::Reflect) - 0.75).abs() < 1e-6);
    }
}
