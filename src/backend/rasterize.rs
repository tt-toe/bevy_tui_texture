use bitvec::order::Lsb0;
use bitvec::slice::BitSlice;
use raqote::{DrawOptions, DrawTarget, SolidSource, Transform};
use rustybuzz::ttf_parser::{GlyphId, RasterGlyphImage, RasterImageFormat, RgbaColor};

use crate::utils::text_atlas::{CacheRect, Entry};
use crate::utils::{Outline, Painter};

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

// ============================================================================
// Fake Bold Rendering
// ============================================================================
//
// Fake bold is achieved by rendering the glyph path multiple times at slight
// horizontal offsets. This creates a thickened appearance that simulates
// bold weight without requiring a dedicated bold font.

/// Default stroke thickness as a fraction of the em-square when the font
/// does not provide underline metrics. Approximately 3% of the em-square.
const DEFAULT_STROKE_THICKNESS_RATIO: f32 = 1.0 / 32.0;

/// Minimum bold offset in pixels to ensure visibility even at small sizes.
const MIN_BOLD_OFFSET_PX: f32 = 0.3;

/// Font weight value for normal (regular) weight fonts.
const FONT_WEIGHT_NORMAL: f32 = 400.0;

/// Font weight delta between normal and bold (700 - 400 = 300).
const FONT_WEIGHT_BOLD_DELTA: f32 = 300.0;

/// Maximum weight factor to prevent excessive bolding for extra-bold fonts.
const MAX_WEIGHT_FACTOR: f32 = 2.0;

/// Horizontal offset ratios for fake bold rendering.
/// Multiple passes at different offsets create a smooth, filled-in appearance.
/// Positive values shift right, negative values shift left.
const BOLD_OFFSET_RATIOS: [f32; 8] = [1.0, 0.5, -0.5, 1.5, 0.25, -0.25, 0.75, -0.75];

/// Calculate the base bold offset based on font metrics.
///
/// The offset is derived from:
/// 1. Font weight - heavier fonts need proportionally less additional bolding
/// 2. Stroke thickness - from underline metrics or a reasonable default
/// 3. Current rendering scale - converts from font units to pixels
///
/// # Arguments
/// * `metrics` - Font face containing weight and metric information
/// * `render_scale` - Current rendering scale (includes 2x supersampling)
///
/// # Returns
/// Base offset in pixels for fake bold rendering
fn calculate_bold_offset(metrics: &rustybuzz::Face, render_scale: f32) -> f32 {
    let font_weight = metrics.weight().to_number() as f32;
    let units_per_em = metrics.units_per_em() as f32;

    // Calculate weight factor: 0.0 for normal weight, scaling up for heavier fonts
    // Clamped to prevent excessive bolding on extra-bold fonts
    let weight_factor = ((font_weight - FONT_WEIGHT_NORMAL) / FONT_WEIGHT_BOLD_DELTA)
        .clamp(0.0, MAX_WEIGHT_FACTOR);

    // Get font-specific stroke thickness from underline metrics if available,
    // otherwise use a reasonable default based on em-square size
    let stroke_thickness = metrics
        .underline_metrics()
        .map(|m| m.thickness as f32)
        .unwrap_or(units_per_em * DEFAULT_STROKE_THICKNESS_RATIO);

    // Convert from font units to pixels, accounting for 2x supersampling
    let scale_to_pixels = render_scale / 2.0;
    let base_offset = stroke_thickness * scale_to_pixels * weight_factor / units_per_em;

    base_offset.max(MIN_BOLD_OFFSET_PX)
}

/// Apply fake bold effect by rendering the path at multiple horizontal offsets.
///
/// # Arguments
/// * `target` - Draw target to render into
/// * `path` - Glyph outline path to render
/// * `base_offset` - Base offset in pixels (will be multiplied by ratios)
fn apply_fake_bold(target: &mut DrawTarget<&mut [u32]>, path: &raqote::Path, base_offset: f32) {
    let white = raqote::Source::Solid(SolidSource::from_unpremultiplied_argb(255, 255, 255, 255));
    let draw_options = DrawOptions::default();

    for ratio in BOLD_OFFSET_RATIOS {
        let offset = base_offset * ratio;
        let bold_transform = Transform::new(1.0, 0.0, 0.0, 1.0, offset, 0.0);
        let transformed_path = path.clone().transform(&bold_transform);
        target.fill(&transformed_path, &white, &draw_options);
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
    actual_width: u32,
    bearing_offset_x: f32, // Horizontal bearing offset from rustybuzz
) -> (CacheRect, Vec<u32>) {
    let scale = cached.width as f32 / actual_width as f32;
    // Apply bearing offset to position glyph within atlas entry
    let computed_offset_x = -(cached.width as f32 * (1.0 - scale)) + bearing_offset_x;
    let computed_offset_y = cached.height as f32 * (1.0 - scale);
    let scale = scale * advance_scale * 2.0;

    let skew = if fake_italic {
        Transform::new(
            /* scale x */ 1.0,
            /* skew x */ 0.0,
            /* skew y */ -0.25,
            /* scale y */ 1.0,
            /* translate x */ -0.25 * cached.width as f32,
            /* translate y */ 0.0,
        )
    } else {
        Transform::default()
    };


    let mut image = vec![0u32; cached.width as usize * 2 * cached.height as usize * 2];
    let mut target = DrawTarget::from_backing(
        cached.width as i32 * 2,
        cached.height as i32 * 2,
        &mut image[..],
    );

    let mut painter = Painter::new(
        metrics,
        &mut target,
        skew,
        scale,
        metrics.ascender() as f32 * scale + computed_offset_y,
        computed_offset_x,
    );
    if metrics
        .paint_color_glyph(
            GlyphId(info.glyph_id as _),
            0,
            RgbaColor::new(255, 255, 255, 255),
            &mut painter,
        )
        .is_some()
    {
        let mut final_image = DrawTarget::new(cached.width as i32, cached.height as i32);
        final_image.draw_image_with_size_at(
            cached.width as f32,
            cached.height as f32,
            0.,
            0.,
            &raqote::Image {
                width: cached.width as i32 * 2,
                height: cached.height as i32 * 2,
                data: &image,
            },
            &DrawOptions {
                blend_mode: raqote::BlendMode::Src,
                antialias: raqote::AntialiasMode::None,
                ..Default::default()
            },
        );

        let mut final_image = final_image.into_vec();
        for argb in final_image.iter_mut() {
            let [a, r, g, b] = argb.to_be_bytes();
            *argb = u32::from_le_bytes([r, g, b, a]);
        }

        return (*cached, final_image);
    }

    if let Some(value) = metrics
        .glyph_raster_image(GlyphId(info.glyph_id as _), u16::MAX)
        .and_then(|raster| extract_color_image(&mut image, raster, cached, advance_scale))
    {
        return value;
    }

    let mut render = Outline::default();
    if let Some(bounds) = metrics.outline_glyph(GlyphId(info.glyph_id as _), &mut render) {
        let path = render.finish();

        // Some fonts return bounds that are entirely negative. I'm not sure why this
        // is, but it means the glyph won't render at all. We check for this here and
        // offset it if so. This seems to let those fonts render correctly.
        let x_off = if bounds.x_max < 0 {
            -bounds.x_min as f32
        } else {
            0.
        };
        let x_off = x_off * scale + computed_offset_x;
        let y_off = metrics.ascender() as f32 * scale + computed_offset_y;

        let mut target = DrawTarget::from_backing(
            cached.width as i32 * 2,
            cached.height as i32 * 2,
            &mut image[..],
        );
        target.set_transform(
            &Transform::scale(scale, -scale)
                .then(&skew)
                .then_translate((x_off, y_off).into()),
        );

        target.fill(
            &path,
            &raqote::Source::Solid(SolidSource::from_unpremultiplied_argb(255, 255, 255, 255)),
            &DrawOptions::default(),
        );

        if fake_bold {
            let bold_offset = calculate_bold_offset(metrics, scale);
            apply_fake_bold(&mut target, &path, bold_offset);
        }

        let mut final_image = DrawTarget::new(cached.width as i32, cached.height as i32);
        final_image.draw_image_with_size_at(
            cached.width as f32,
            cached.height as f32,
            0.,
            0.,
            &raqote::Image {
                width: cached.width as i32 * 2,
                height: cached.height as i32 * 2,
                data: &image,
            },
            &DrawOptions {
                blend_mode: raqote::BlendMode::Src,
                antialias: raqote::AntialiasMode::None,
                ..Default::default()
            },
        );

        return (*cached, final_image.into_vec());
    }

    if let Some(value) = metrics
        .glyph_raster_image(GlyphId(info.glyph_id as _), u16::MAX)
        .and_then(|raster| extract_bw_image(&mut image, raster, cached, advance_scale))
    {
        return value;
    }

    (
        *cached,
        vec![0u32; cached.width as usize * cached.height as usize],
    )
}

fn extract_color_image(
    image: &mut Vec<u32>,
    raster: RasterGlyphImage,
    cached: Entry,
    scale: f32,
) -> Option<(CacheRect, Vec<u32>)> {
    match raster.format {
        RasterImageFormat::PNG => {
            // PNG format not supported (simplified implementation)
            return None;
        }
        RasterImageFormat::BitmapPremulBgra32 => {
            image.resize(raster.width as usize * raster.height as usize, 0);
            for (y, row) in raster.data.chunks(raster.width as usize * 4).enumerate() {
                for (x, pixel) in row.chunks(4).enumerate() {
                    let pixel: &[u8; 4] = pixel.try_into().expect("Invalid chunk size");
                    let [b, g, r, a] = *pixel;
                    let pixel = u32::from_be_bytes([a, r, g, b]);
                    image[y * raster.width as usize + x] = pixel;
                }
            }
        }
        _ => return None,
    }

    let mut final_image = DrawTarget::new(cached.width as i32, cached.height as i32);
    final_image.draw_image_with_size_at(
        cached.width as f32,
        cached.height as f32,
        raster.x as f32 * scale,
        raster.y as f32 * scale,
        &raqote::Image {
            width: raster.width as i32,
            height: raster.height as i32,
            data: &*image,
        },
        &DrawOptions {
            blend_mode: raqote::BlendMode::Src,
            antialias: raqote::AntialiasMode::None,
            ..Default::default()
        },
    );

    let mut final_image = final_image.into_vec();
    for argb in final_image.iter_mut() {
        let [a, r, g, b] = argb.to_be_bytes();
        *argb = u32::from_le_bytes([r, g, b, a]);
    }

    Some((*cached, final_image))
}

fn extract_bw_image(
    image: &mut Vec<u32>,
    raster: RasterGlyphImage,
    cached: Entry,
    scale: f32,
) -> Option<(CacheRect, Vec<u32>)> {
    image.resize(raster.width as usize * raster.height as usize, 0);

    match raster.format {
        RasterImageFormat::BitmapMono => {
            from_gray_unpacked::<1, 2>(image, raster, LUT_1);
        }
        RasterImageFormat::BitmapMonoPacked => {
            from_gray_packed::<1, 2>(image, raster, LUT_1);
        }
        RasterImageFormat::BitmapGray2 => {
            from_gray_unpacked::<2, 4>(image, raster, LUT_2);
        }
        RasterImageFormat::BitmapGray2Packed => {
            from_gray_packed::<2, 4>(image, raster, LUT_2);
        }
        RasterImageFormat::BitmapGray4 => {
            from_gray_unpacked::<4, 16>(image, raster, LUT_4);
        }
        RasterImageFormat::BitmapGray4Packed => {
            from_gray_packed::<4, 16>(image, raster, LUT_4);
        }
        RasterImageFormat::BitmapGray8 => {
            for (byte, dst) in raster.data.iter().zip(image.iter_mut()) {
                *dst = u32::from_be_bytes([*byte, 255, 255, 255]);
            }
        }
        _ => return None,
    }

    let mut final_image = DrawTarget::new(cached.width as i32, cached.height as i32);
    final_image.draw_image_with_size_at(
        cached.width as f32,
        cached.height as f32,
        raster.x as f32 * scale,
        raster.y as f32 * scale,
        &raqote::Image {
            width: raster.width as i32,
            height: raster.height as i32,
            data: &*image,
        },
        &DrawOptions {
            blend_mode: raqote::BlendMode::Src,
            antialias: raqote::AntialiasMode::None,
            ..Default::default()
        },
    );

    let mut final_image = final_image.into_vec();
    for argb in final_image.iter_mut() {
        let [a, r, g, b] = argb.to_be_bytes();
        *argb = u32::from_le_bytes([r, g, b, a]);
    }

    Some((*cached, final_image))
}

fn from_gray_unpacked<const BITS: usize, const ENTRIES: usize>(
    image: &mut [u32],
    raster: RasterGlyphImage,
    steps: [u8; ENTRIES],
) {
    for (bits, dst) in raster
        .data
        .chunks((raster.width as usize / (8 / BITS)) + 1)
        .zip(image.chunks_mut(raster.width as usize))
    {
        let bits = BitSlice::<_, Lsb0>::from_slice(bits);
        for (bits, dst) in bits.chunks(BITS).zip(dst.iter_mut()) {
            let mut index = 0;
            for idx in bits.iter_ones() {
                index |= 1 << (BITS - idx - 1);
            }
            let value = steps[index as usize];
            *dst = u32::from_be_bytes([value, 255, 255, 255]);
        }
    }
}

fn from_gray_packed<const BITS: usize, const ENTRIES: usize>(
    image: &mut [u32],
    raster: RasterGlyphImage,
    steps: [u8; ENTRIES],
) {
    let bits = BitSlice::<_, Lsb0>::from_slice(raster.data);
    for (bits, dst) in bits.chunks(BITS).zip(image.iter_mut()) {
        let mut index = 0;
        for idx in bits.iter_ones() {
            index |= 1 << (BITS - idx - 1);
        }
        let value = steps[index as usize];
        *dst = u32::from_be_bytes([value, 255, 255, 255]);
    }
}
