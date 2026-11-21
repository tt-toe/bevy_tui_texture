use rustybuzz::ttf_parser::{GlyphId, RasterGlyphImage, RasterImageFormat};

use crate::backend::glyph_constraint::{CellMetrics, GlyphConstraint, GlyphRect};
use crate::utils::colr::{self, Affine, ColorPainter};
use crate::utils::outline::{OutlineSink, map_points};
use crate::utils::text_atlas::{CacheRect, Entry};

const LUT_1: [u8; 2] = [0, 255];
const LUT_2: [u8; 4] = [0, 255 / 3, 2 * (255 / 3), 255];
const LUT_4: [u8; 16] = [
    0,
    255 / 15,
    2 * (255 / 15),
    3 * (255 / 15),
    4 * (255 / 15),
    5 * (255 / 15),
    6 * (255 / 15),
    7 * (255 / 15),
    8 * (255 / 15),
    9 * (255 / 15),
    10 * (255 / 15),
    11 * (255 / 15),
    12 * (255 / 15),
    13 * (255 / 15),
    14 * (255 / 15),
    255,
];

/// Fake bold coverage: `1 - (1 - a)^9`, what nine stacked fills of the
/// same edge add up to.
const BOLD_LUT: [u8; 256] = {
    let mut lut = [0u8; 256];
    let d: u128 = 255u128.pow(8);
    let mut a = 0;
    while a < 256 {
        let n = (255 - a as u128).pow(9);
        lut[a] = (255 - (n + d / 2) / d) as u8;
        a += 1;
    }
    lut
};

/// Buffers reused across glyph rasterizations of one `Fonts`.
#[derive(Default)]
pub(crate) struct RasterScratch {
    zeno: zeno::Scratch,
    path: Vec<zeno::Command>,
    canvas: Vec<u8>,
}

/// Font units -> canvas pixels: `X = sx*x + skew*y + tx`, `Y = ty - sy*y`,
/// all multiplied by `scale`.
#[derive(Clone, Copy)]
struct PointMap {
    sx: f32,
    skew: f32,
    tx: f32,
    sy: f32,
    ty: f32,
    scale: f32,
}

impl PointMap {
    fn apply(&self, p: zeno::Vector) -> zeno::Vector {
        zeno::Vector::new(
            (self.sx * p.x + self.skew * p.y + self.tx) * self.scale,
            (self.ty - self.sy * p.y) * self.scale,
        )
    }
}

/// Coverage of `path` on a zeroed `width` x `height` canvas.
fn fill_mask(
    path: &[zeno::Command],
    scratch: &mut zeno::Scratch,
    width: usize,
    height: usize,
    out: &mut [u8],
) {
    zeno::Mask::with_scratch(path, scratch)
        .style(zeno::Fill::NonZero)
        .size(width as u32, height as u32)
        .render_into(out, None);
}

/// Where a glyph lands in its atlas slot.
#[derive(Debug, Clone, Copy)]
pub(super) enum Placement<'a> {
    /// Where its font puts it: baseline at the font's ascender, shifted by
    /// the shaper's `bearing_offset_x` (pixels) and squeezed horizontally
    /// by `fit_x` (<= 1, see `fit_to_cells`).
    Natural { bearing_offset_x: f32, fit_x: f32 },
    /// Scaled and aligned into the slot's `cells` cells by a symbol
    /// constraint.
    Constrained {
        constraint: &'a GlyphConstraint,
        metrics: CellMetrics,
        cells: u32,
    },
}

/// Font units -> pixels of the 2x supersampled canvas:
/// `(x * sx + tx, -y * sy + ty)`.
struct CanvasTransform {
    sx: f32,
    sy: f32,
    tx: f32,
    ty: f32,
}

impl CanvasTransform {
    fn new(
        face: &rustybuzz::Face,
        glyph: GlyphId,
        advance_scale: f32,
        cell_height: f32,
        placement: Placement,
    ) -> Self {
        let natural = |bearing_offset_x: f32, fit_x: f32| Self {
            sx: advance_scale * 2.0 * fit_x,
            sy: advance_scale * 2.0,
            tx: bearing_offset_x,
            ty: face.ascender() as f32 * advance_scale * 2.0,
        };
        let (constraint, metrics, cells) = match placement {
            Placement::Natural {
                bearing_offset_x,
                fit_x,
            } => return natural(bearing_offset_x, fit_x),
            Placement::Constrained {
                constraint,
                metrics,
                cells,
            } => (constraint, metrics, cells),
        };
        // Constraints work on the ink box; a glyph without ink has nothing
        // to place.
        let Some(bbox) = face
            .glyph_bounding_box(glyph)
            .filter(|b| b.x_max > b.x_min && b.y_max > b.y_min)
        else {
            return natural(0.0, 1.0);
        };
        let (x_min, x_max) = (bbox.x_min as f32, bbox.x_max as f32);
        let (y_min, y_max) = (bbox.y_min as f32, bbox.y_max as f32);
        let ascender = face.ascender() as f32;
        let ink = GlyphRect {
            x: x_min * advance_scale,
            y: cell_height - (ascender - y_min) * advance_scale,
            width: (x_max - x_min) * advance_scale,
            height: (y_max - y_min) * advance_scale,
        };
        let placed = constraint.constrain(ink, &metrics, cells);
        let (kx, ky) = (placed.width / ink.width, placed.height / ink.height);
        let top = cell_height - (placed.y + placed.height);
        Self {
            sx: advance_scale * 2.0 * kx,
            sy: advance_scale * 2.0 * ky,
            tx: 2.0 * (placed.x - x_min * advance_scale * kx),
            ty: 2.0 * (top + y_max * advance_scale * ky),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn rasterize_glyph(
    cached: Entry,
    metrics: &rustybuzz::Face,
    info: &rustybuzz::GlyphInfo,
    fake_italic: bool,
    fake_bold: bool,
    advance_scale: f32,
    placement: Placement,
    scratch: &mut RasterScratch,
) -> (CacheRect, Vec<u8>) {
    let natural = matches!(placement, Placement::Natural { .. });
    let CanvasTransform { sx, sy, tx, ty } = CanvasTransform::new(
        metrics,
        GlyphId(info.glyph_id as _),
        advance_scale,
        cached.height as f32,
        placement,
    );

    let glyph_id = GlyphId(info.glyph_id as _);
    let (w, h) = (cached.width as usize, cached.height as usize);
    let italic = if fake_italic { 0.25 } else { 0.0 };
    if metrics.is_color_glyph(glyph_id) {
        let base = Affine::new(sx, 0.0, italic * sy, -sy, tx - italic * w as f32, ty)
            .then(&Affine::scale(0.5, 0.5));
        if let Some(pixels) = ColorPainter::new(metrics, w, h, base).paint_glyph(glyph_id) {
            return (*cached, colr::to_alpha(&pixels));
        }
    }

    if let Some(mask) = metrics
        .glyph_raster_image(glyph_id, u16::MAX)
        .filter(|raster| raster.format == RasterImageFormat::BitmapPremulBgra32)
        .and_then(|raster| bitmap_coverage(raster, cached, advance_scale))
    {
        return (*cached, mask);
    }

    scratch.path.clear();
    if let Some(bounds) = metrics.outline_glyph(glyph_id, &mut OutlineSink(&mut scratch.path)) {
        let x_off = if natural && bounds.x_max < 0 {
            -bounds.x_min as f32
        } else {
            0.
        };
        let map = PointMap {
            sx,
            skew: italic * sy,
            tx: x_off * sx + tx - italic * w as f32,
            sy,
            ty,
            scale: if fake_bold { 1.0 } else { 0.5 },
        };
        let mut out = vec![0u8; w * h];
        if !map_points(&mut scratch.path, |p| map.apply(p)) {
            return (*cached, out);
        }
        if !fake_bold {
            fill_mask(&scratch.path, &mut scratch.zeno, w, h, &mut out);
            return (*cached, out);
        }
        let (cw, ch) = (w * 2, h * 2);
        scratch.canvas.clear();
        scratch.canvas.resize(cw * ch, 0);
        fill_mask(
            &scratch.path,
            &mut scratch.zeno,
            cw,
            ch,
            &mut scratch.canvas,
        );
        for (y, row) in out.chunks_exact_mut(w.max(1)).enumerate() {
            let top = scratch.canvas.get(2 * y * cw..(2 * y + 1) * cw);
            let bottom = scratch.canvas.get((2 * y + 1) * cw..(2 * y + 2) * cw);
            let (Some(top), Some(bottom)) = (top, bottom) else {
                break;
            };
            for (x, dst) in row.iter_mut().enumerate() {
                let sum: u32 = [
                    top.get(2 * x),
                    top.get(2 * x + 1),
                    bottom.get(2 * x),
                    bottom.get(2 * x + 1),
                ]
                .into_iter()
                .flatten()
                .filter_map(|&a| BOLD_LUT.get(usize::from(a)))
                .map(|&a| u32::from(a))
                .sum();
                *dst = (sum / 4) as u8;
            }
        }
        return (*cached, out);
    }

    if let Some(mask) = metrics
        .glyph_raster_image(glyph_id, u16::MAX)
        .and_then(|raster| bitmap_coverage(raster, cached, advance_scale))
    {
        return (*cached, mask);
    }

    (*cached, vec![0u8; w * h])
}

/// A color glyph as premultiplied RGBA8 (`width * height * 4` bytes), or
/// `None` if the font has no color data it can draw for it: COLR (v0 and
/// v1), then a PNG strike (CBDT, sbix), then a premultiplied BGRA strike.
#[cfg(feature = "emoji")]
pub(super) fn rasterize_color_glyph(
    face: &rustybuzz::Face,
    glyph: GlyphId,
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    let (w, h) = (width as usize, height as usize);
    if face.is_color_glyph(glyph) {
        let ascender = face.ascender() as f32;
        let line_height = (face.ascender() - face.descender()).max(1) as f32;
        let advance = face
            .glyph_hor_advance(glyph)
            .filter(|&advance| advance > 0)
            .map_or(face.units_per_em() as f32, f32::from);
        let scale = (width as f32 / advance).min(height as f32 / line_height);
        let x_offset = (width as f32 - advance * scale) / 2.0;
        let y_offset = (height as f32 - line_height * scale) / 2.0 + ascender * scale;
        let base = Affine::new(scale, 0.0, 0.0, -scale, x_offset, y_offset);
        if let Some(pixels) = ColorPainter::new(face, w, h, base).paint_glyph(glyph) {
            return Some(colr::to_rgba8(&pixels));
        }
    }

    let raster =
        face.glyph_raster_image(glyph, (2 * width.max(height)).min(u16::MAX as u32) as u16)?;
    if raster_is_empty(&raster) {
        return None;
    }
    let (pixels, pw, ph) = match raster.format {
        RasterImageFormat::PNG => decode_png(raster.data)?,
        RasterImageFormat::BitmapPremulBgra32 => {
            let (pw, ph) = (raster.width as usize, raster.height as usize);
            let bgra = raster.data.get(..pw * ph * 4)?;
            let rgba = bgra
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|&[b, g, r, a]| [r, g, b, a])
                .collect();
            (rgba, pw, ph)
        }
        _ => return None,
    };
    if pw == 0 || ph == 0 || w == 0 || h == 0 {
        return None;
    }

    let fit = (w as f32 / pw as f32).min(h as f32 / ph as f32);
    let fit_w = ((pw as f32 * fit).round() as usize).clamp(1, w);
    let fit_h = ((ph as f32 * fit).round() as usize).clamp(1, h);
    let scaled = box_down(&pixels, pw, ph, fit_w, fit_h);
    let mut out = vec![0u8; w * h * 4];
    let (x0, y0) = ((w - fit_w) / 2, (h - fit_h) / 2);
    for (row, line) in scaled.chunks_exact(fit_w * 4).enumerate() {
        let start = ((y0 + row) * w + x0) * 4;
        if let Some(dst) = out.get_mut(start..start + line.len()) {
            dst.copy_from_slice(line);
        }
    }
    Some(out)
}

/// RGBA8 `src` (`sw` x `sh`) resampled to `dw` x `dh` by averaging the
/// source pixels each destination pixel covers.
#[cfg(feature = "emoji")]
fn box_down(src: &[u8], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<u8> {
    let mut out = vec![0u8; dw * dh * 4];
    for y in 0..dh {
        let (y0, y1) = (y * sh / dh, ((y + 1) * sh).div_ceil(dh).min(sh));
        for x in 0..dw {
            let (x0, x1) = (x * sw / dw, ((x + 1) * sw).div_ceil(dw).min(sw));
            let (mut sum, mut n) = ([0u32; 4], 0u32);
            for yy in y0..y1 {
                for xx in x0..x1 {
                    let i = (yy * sw + xx) * 4;
                    if let Some(&[r, g, b, a]) = src.get(i..i + 4) {
                        for (sum, value) in sum.iter_mut().zip([r, g, b, a]) {
                            *sum += u32::from(value);
                        }
                        n += 1;
                    }
                }
            }
            let i = (y * dw + x) * 4;
            if let Some(dst) = out.get_mut(i..i + 4) {
                for (dst, sum) in dst.iter_mut().zip(sum) {
                    *dst = (sum / n.max(1)) as u8;
                }
            }
        }
    }
    out
}

/// An embedded bitmap with no pixels - what a bitmap strike stores for a
/// blank glyph such as the space.
fn raster_is_empty(raster: &RasterGlyphImage) -> bool {
    raster.width == 0 || raster.height == 0
}

/// An embedded bitmap (BGRA, grayscale or monochrome) as coverage,
/// stretched over the slot (see [`resample_alpha`]).
fn bitmap_coverage(raster: RasterGlyphImage, cached: Entry, scale: f32) -> Option<Vec<u8>> {
    if raster_is_empty(&raster) {
        return None;
    }
    let (sw, sh) = (raster.width as usize, raster.height as usize);
    let mut alpha = vec![0u8; sw * sh];
    match raster.format {
        RasterImageFormat::BitmapPremulBgra32 => {
            for (dst, &[_, _, _, a]) in alpha.iter_mut().zip(raster.data.as_chunks::<4>().0) {
                *dst = a;
            }
        }
        RasterImageFormat::BitmapMono => unpack_rows::<1, 2>(&mut alpha, raster, LUT_1),
        RasterImageFormat::BitmapMonoPacked => unpack_gray(raster.data, &LUT_1, 1, &mut alpha),
        RasterImageFormat::BitmapGray2 => unpack_rows::<2, 4>(&mut alpha, raster, LUT_2),
        RasterImageFormat::BitmapGray2Packed => unpack_gray(raster.data, &LUT_2, 2, &mut alpha),
        RasterImageFormat::BitmapGray4 => unpack_rows::<4, 16>(&mut alpha, raster, LUT_4),
        RasterImageFormat::BitmapGray4Packed => unpack_gray(raster.data, &LUT_4, 4, &mut alpha),
        RasterImageFormat::BitmapGray8 => {
            for (dst, &byte) in alpha.iter_mut().zip(raster.data) {
                *dst = byte;
            }
        }
        _ => return None,
    }
    Some(resample_alpha(&alpha, (sw, sh), cached, raster, scale))
}

/// `src` (`sw` x `sh`) stretched bilinearly over the whole slot, shifted by
/// the bitmap's bearing `(raster.x, raster.y) * scale`; pixels whose center
/// falls outside the stretched image stay 0.
fn resample_alpha(
    src: &[u8],
    (sw, sh): (usize, usize),
    cached: Entry,
    raster: RasterGlyphImage,
    scale: f32,
) -> Vec<u8> {
    let (w, h) = (cached.width as usize, cached.height as usize);
    let (dx, dy) = (f32::from(raster.x) * scale, f32::from(raster.y) * scale);
    let mut out = vec![0u8; w * h];
    if sw == 0 || sh == 0 || w == 0 || h == 0 {
        return out;
    }
    let at = |x: usize, y: usize| f32::from(src.get(y * sw + x).copied().unwrap_or(0));
    for (i, dst) in out.iter_mut().enumerate() {
        let (px, py) = ((i % w) as f32 + 0.5 - dx, (i / w) as f32 + 0.5 - dy);
        if px < 0.0 || py < 0.0 || px >= w as f32 || py >= h as f32 {
            continue;
        }
        let u = (px * sw as f32 / w as f32 - 0.5).clamp(0.0, (sw - 1) as f32);
        let v = (py * sh as f32 / h as f32 - 0.5).clamp(0.0, (sh - 1) as f32);
        let (x0, y0) = (u.floor() as usize, v.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(sw - 1), (y0 + 1).min(sh - 1));
        let (fx, fy) = (u - x0 as f32, v - y0 as f32);
        let top = at(x0, y0) * (1.0 - fx) + at(x1, y0) * fx;
        let bottom = at(x0, y1) * (1.0 - fx) + at(x1, y1) * fx;
        *dst = (top * (1.0 - fy) + bottom * fy).round() as u8;
    }
    out
}

/// One `bits`-wide gray level per pixel of `dst`, mapped through `steps`.
fn unpack_gray(data: &[u8], steps: &[u8], width: usize, dst: &mut [u8]) {
    let width = width.max(1);
    let total_bits = data.len() * 8;
    for (pixel, dst) in dst.iter_mut().enumerate() {
        let first = pixel * width;
        if first >= total_bits {
            break;
        }
        let mut index = 0usize;
        for j in 0..width {
            let bit = first + j;
            let set = data
                .get(bit / 8)
                .is_some_and(|byte| byte >> (bit % 8) & 1 == 1);
            if set {
                index |= 1 << width.saturating_sub(j + 1);
            }
        }
        *dst = steps.get(index).copied().unwrap_or(0);
    }
}

/// [`unpack_gray`] for bitmaps whose rows start on a byte boundary.
fn unpack_rows<const BITS: usize, const ENTRIES: usize>(
    dst: &mut [u8],
    raster: RasterGlyphImage,
    steps: [u8; ENTRIES],
) {
    let row_bytes = usize::from(raster.width) / (8 / BITS).max(1) + 1;
    for (bits, dst) in raster
        .data
        .chunks(row_bytes)
        .zip(dst.chunks_mut(usize::from(raster.width).max(1)))
    {
        unpack_gray(bits, &steps, BITS, dst);
    }
}

/// A PNG strike as premultiplied RGBA8 and its size.
#[cfg(feature = "emoji")]
fn decode_png(data: &[u8]) -> Option<(Vec<u8>, usize, usize)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let pixels = buf.get(..info.buffer_size())?;
    let rgba: Vec<[u8; 4]> = match info.color_type {
        png::ColorType::Rgba => pixels.as_chunks::<4>().0.to_vec(),
        png::ColorType::Rgb => pixels
            .as_chunks::<3>()
            .0
            .iter()
            .map(|&[r, g, b]| [r, g, b, 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => pixels
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[l, a]| [l, l, l, a])
            .collect(),
        png::ColorType::Grayscale => pixels.iter().map(|&l| [l, l, l, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    let premultiplied = rgba
        .iter()
        .flat_map(|&[r, g, b, a]| {
            let m = |c: u8| ((u16::from(c) * u16::from(a) + 127) / 255) as u8;
            [m(r), m(g), m(b), a]
        })
        .collect();
    Some((premultiplied, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raster(width: u16, height: u16, format: RasterImageFormat) -> RasterGlyphImage<'static> {
        RasterGlyphImage {
            x: 0,
            y: 0,
            width,
            height,
            pixels_per_em: 16,
            format,
            data: &[],
        }
    }

    #[test]
    fn a_zero_size_bitmap_glyph_has_no_image() {
        let slot = Entry::Uncached(CacheRect {
            x: 0,
            y: 0,
            width: 8,
            height: 16,
        });
        let formats = [
            RasterImageFormat::BitmapMono,
            RasterImageFormat::BitmapMonoPacked,
            RasterImageFormat::BitmapGray2,
            RasterImageFormat::BitmapGray2Packed,
            RasterImageFormat::BitmapGray4,
            RasterImageFormat::BitmapGray4Packed,
            RasterImageFormat::BitmapGray8,
            RasterImageFormat::BitmapPremulBgra32,
        ];
        for format in formats {
            for (width, height) in [(0, 0), (0, 12), (7, 0)] {
                assert!(bitmap_coverage(raster(width, height, format), slot, 1.0).is_none());
            }
        }
    }

    use crate::fonts::{Font, Fonts};

    const MPLUS: &[u8] = crate::test_fonts::MPLUS;
    const FUSION: &[u8] = crate::test_fonts::FUSION;
    const SAMPLE: &str = "AgWm@0#iéΩあアンぱ漢字鬱東한글";

    struct Sampled {
        glyph: GlyphId,
        size: (usize, usize),
        advance_scale: f32,
    }

    /// The sample characters the font has, sized as `shape_row` slots them.
    fn samples(font: &Font, size: u16) -> Vec<Sampled> {
        let fonts = Fonts::new(font.clone(), size.into());
        let face = font.font();
        let (cell_w, cell_h) = (fonts.min_width_px() as usize, fonts.height_px() as usize);
        let advance_scale = cell_h as f32 / face.height() as f32;
        let found: Vec<_> = SAMPLE
            .chars()
            .filter_map(|c| {
                let glyph = face.glyph_index(c)?;
                let cells = if (c as u32) >= 0x1100 { 2 } else { 1 };
                Some(Sampled {
                    glyph,
                    size: (cell_w * cells, cell_h),
                    advance_scale,
                })
            })
            .collect();
        assert!(found.len() >= 15, "only {} sample glyphs", found.len());
        found
    }

    fn zeno_render(face: &rustybuzz::Face, s: &Sampled, fake_bold: bool) -> Vec<u8> {
        let slot = Entry::Uncached(CacheRect {
            x: 0,
            y: 0,
            width: s.size.0 as u32,
            height: s.size.1 as u32,
        });
        let mut info = rustybuzz::GlyphInfo::default();
        info.glyph_id = u32::from(s.glyph.0);
        let placement = Placement::Natural {
            bearing_offset_x: 0.0,
            fit_x: 1.0,
        };
        let mut scratch = RasterScratch::default();
        let (_, mask) = rasterize_glyph(
            slot,
            face,
            &info,
            false,
            fake_bold,
            s.advance_scale,
            placement,
            &mut scratch,
        );
        mask
    }

    fn fnv1a(hash: u64, bytes: &[u8]) -> u64 {
        bytes
            .iter()
            .fold(hash, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3))
    }

    #[test]
    fn bold_lut_spans_the_range_monotonically() {
        assert_eq!(BOLD_LUT[0], 0);
        assert_eq!(BOLD_LUT[255], 255);
        assert!(BOLD_LUT.windows(2).all(|w| w[0] <= w[1]));
    }

    /// Reference points of `scale(sx, -sy).then(skew).then_translate(..)`.
    #[test]
    fn point_map_matches_the_reference_transform() {
        let (sx, sy, tx, ty, x_off, width) = (0.031f32, 0.029f32, 1.5f32, 27.0f32, 0.75f32, 9.0f32);
        let cases = [
            (false, (0.0, 0.0), (1.52325, 27.0)),
            (false, (512.0, -200.0), (17.39525, 32.8)),
            (false, (-30.0, 880.0), (0.59325, 1.48)),
            (true, (0.0, 0.0), (-0.72675, 27.0)),
            (true, (512.0, -200.0), (13.69525, 32.8)),
            (true, (-30.0, 880.0), (4.72325, 1.48)),
        ];
        for (italic, (x, y), (wx, wy)) in cases {
            let k = if italic { 0.25 } else { 0.0 };
            let map = PointMap {
                sx,
                skew: k * sy,
                tx: x_off * sx + tx - k * width,
                sy,
                ty,
                scale: 1.0,
            };
            let got = map.apply(zeno::Vector::new(x, y));
            assert!((got.x - wx).abs() < 1e-3, "{italic} x {x},{y}: {}", got.x);
            assert!((got.y - wy).abs() < 1e-3, "{italic} y {x},{y}: {}", got.y);
        }
    }

    #[test]
    fn golden_masks() {
        let mut hash = 0xcbf29ce484222325;
        for data in [MPLUS, FUSION] {
            let font = Font::new(data).expect("font");
            for size in [16, 20, 32] {
                for s in samples(&font, size) {
                    for bold in [false, true] {
                        hash = fnv1a(hash, &zeno_render(font.font(), &s, bold));
                    }
                }
            }
        }
        assert_eq!(hash, GOLDEN, "{hash:#x}");
    }

    const COLRV1: &[u8] = crate::test_fonts::COLRV1;

    fn color_glyphs(face: &rustybuzz::Face) -> Vec<GlyphId> {
        (0..face.number_of_glyphs())
            .map(GlyphId)
            .filter(|&g| face.is_color_glyph(g))
            .collect()
    }

    #[test]
    fn golden_colr_silhouettes() {
        let face = rustybuzz::Face::from_slice(COLRV1, 0).expect("fixture");
        let glyphs = color_glyphs(&face);
        assert_eq!(glyphs.len(), 40);
        let mut hash = 0xcbf29ce484222325;
        for glyph in glyphs {
            let slot = Entry::Uncached(CacheRect {
                x: 0,
                y: 0,
                width: 16,
                height: 16,
            });
            let mut info = rustybuzz::GlyphInfo::default();
            info.glyph_id = u32::from(glyph.0);
            let (_, mask) = rasterize_glyph(
                slot,
                &face,
                &info,
                false,
                false,
                16.0 / face.height() as f32,
                Placement::Natural {
                    bearing_offset_x: 0.0,
                    fit_x: 1.0,
                },
                &mut RasterScratch::default(),
            );
            assert!(mask.iter().any(|&a| a > 0), "{glyph:?} is blank");
            hash = fnv1a(hash, &mask);
        }
        assert_eq!(hash, GOLDEN_SILHOUETTES, "{hash:#x}");
    }

    #[cfg(feature = "emoji")]
    #[test]
    fn golden_colr_color_glyphs() {
        let face = rustybuzz::Face::from_slice(COLRV1, 0).expect("fixture");
        let mut hash = 0xcbf29ce484222325;
        for glyph in color_glyphs(&face) {
            for size in [16, 32] {
                let rgba = rasterize_color_glyph(&face, glyph, size, size).expect("COLR glyph");
                hash = fnv1a(hash, &rgba);
            }
        }
        assert_eq!(hash, GOLDEN_COLOR, "{hash:#x}");
    }

    #[cfg(feature = "emoji")]
    #[test]
    fn png_strikes_decode_to_premultiplied_rgba() {
        let data = crate::test_fonts::CBDT;
        let face = rustybuzz::Face::from_slice(data, 0).expect("fixture");
        let mut decoded = 0;
        for g in 0..face.number_of_glyphs() {
            let Some(raster) = face.glyph_raster_image(GlyphId(g), u16::MAX) else {
                continue;
            };
            if raster.format != RasterImageFormat::PNG {
                continue;
            }
            let (rgba, w, h) = decode_png(raster.data).expect("png");
            assert_eq!(rgba.len(), w * h * 4);
            assert!(
                rgba.as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| p[..3].iter().all(|&c| c <= p[3]))
            );
            decoded += 1;
        }
        assert_eq!(decoded, 40);
    }

    #[test]
    fn a_bitmap_at_its_own_size_is_copied_unchanged() {
        let alpha: Vec<u8> = (0..8 * 16).map(|i| (i * 37 % 256) as u8).collect();
        let raster = RasterGlyphImage {
            x: 0,
            y: 0,
            width: 8,
            height: 16,
            pixels_per_em: 16,
            format: RasterImageFormat::BitmapGray8,
            data: &alpha,
        };
        let slot = Entry::Uncached(CacheRect {
            x: 0,
            y: 0,
            width: 8,
            height: 16,
        });
        assert_eq!(bitmap_coverage(raster, slot, 1.0), Some(alpha));
    }

    const GOLDEN_SILHOUETTES: u64 = 0x88f8e39f5413851d;
    #[cfg(feature = "emoji")]
    const GOLDEN_COLOR: u64 = 0xa2e3ccec8b1c7b79;
    const GOLDEN: u64 = 0x9b270c0506332039;
}
